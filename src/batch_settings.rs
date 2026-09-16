use crate::app_state::{BaseColor, PipelineState};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

pub const GEOMETRY_MODULE: &str = "geometry";
pub const FILM_AREA_MODULE: &str = "film_area";
pub const FILM_BASE_MODULE: &str = "base_color";

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ImageKey {
    pub roll_id: String,
    pub file_path: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BatchCopyResult {
    pub updated: usize,
    pub targets: Vec<ImageKey>,
    pub modules: Vec<String>,
}

#[derive(Debug)]
pub struct BatchCopyCommit {
    pub result: BatchCopyResult,
    pub geometry: Option<Value>,
    /// The source frame's film base, used as a place-holder only on targets that
    /// have no measurement of their own. Every target keeps its own pipeline
    /// state and re-measures the base as soon as it is decoded.
    pub inherited_base_color: Option<String>,
    /// Targets that received the place-holder base.
    pub inherited_targets: Vec<ImageKey>,
}

/// Copy selected JSON modules in one transaction. This function only mutates
/// persisted edit state; it has no access to image decoders or render buffers.
pub fn copy_settings_transaction(
    connection: &mut Connection,
    source: &ImageKey,
    targets: &[ImageKey],
    modules: &[String],
    updated_at: i64,
) -> Result<BatchCopyCommit, String> {
    validate_modules(modules)?;

    let mut seen = HashSet::new();
    let targets = targets
        .iter()
        .filter(|target| *target != source && seen.insert((*target).clone()))
        .cloned()
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Ok(BatchCopyCommit {
            result: BatchCopyResult {
                updated: 0,
                targets,
                modules: modules.to_vec(),
            },
            geometry: None,
            inherited_base_color: None,
            inherited_targets: Vec::new(),
        });
    }

    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to start settings transaction: {error}"))?;
    let source_geometry = if modules.iter().any(|module| module == GEOMETRY_MODULE) {
        Some(read_json_column(&transaction, source, "geom", "source")?)
    } else if modules.iter().any(|module| module == FILM_AREA_MODULE) {
        let geometry = read_json_column(&transaction, source, "geom", "source")?;
        let points = geometry
            .get("calibration_points")
            .cloned()
            .unwrap_or(Value::Null);
        let confirmed = geometry
            .get("calibration_confirmed")
            .cloned()
            .unwrap_or(Value::Bool(false));
        Some(serde_json::json!({
            "calibration_points": points,
            "calibration_confirmed": confirmed,
        }))
    } else {
        None
    };

    let source_film_base: Option<String> =
        if modules.iter().any(|module| module == FILM_BASE_MODULE) {
            transaction
                .query_row(
                    "SELECT base_color FROM image_states WHERE roll_id = ?1 AND file_path = ?2",
                    params![source.roll_id, source.file_path],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| format!("Failed to read source base state: {error}"))?
                .flatten()
        } else {
            None
        };
    let source_base_color = source_film_base.as_deref().filter(|base| !base.is_empty());
    let mut inherited_targets = Vec::new();

    for target in &targets {
        if let Some(source_geometry) = source_geometry.as_ref() {
            let mut target_geometry = read_json_column(&transaction, target, "geom", "target")?;
            merge_json(&mut target_geometry, source_geometry);
            let serialized = serde_json::to_string(&target_geometry)
                .map_err(|error| format!("Failed to serialize target geometry: {error}"))?;
            let updated = transaction
                .execute(
                    "UPDATE image_states
                     SET geom = ?1, updated_at = ?2
                     WHERE roll_id = ?3 AND file_path = ?4",
                    params![serialized, updated_at, target.roll_id, target.file_path],
                )
                .map_err(|error| {
                    format!(
                        "Failed to update settings for {}/{}: {error}",
                        target.roll_id, target.file_path
                    )
                })?;
            if updated != 1 {
                return Err(format!(
                    "Target image state disappeared during batch update: {}/{}",
                    target.roll_id, target.file_path
                ));
            }
        }
        if let Some(base_color) = source_base_color {
            let (target_base_color, target_pipeline_state): (Option<String>, Option<String>) =
                transaction
                    .query_row(
                        "SELECT base_color, pipeline_state FROM image_states
                         WHERE roll_id = ?1 AND file_path = ?2",
                        params![target.roll_id, target.file_path],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .map_err(|error| format!("Failed to read target base state: {error}"))?
                    .unwrap_or((None, None));
            // A frame that already measured its own film keeps that
            // measurement: the recipe travels, the physical reading does not.
            if !target_keeps_its_own_base(
                target_base_color.as_deref(),
                target_pipeline_state.as_deref(),
            ) {
                let pipeline_state = target_pipeline_state.ok_or_else(|| {
                    format!(
                        "Target image state has no pipeline state: {}/{}",
                        target.roll_id, target.file_path
                    )
                })?;
                // Only the provenance changes: the target keeps its own
                // contract, anchors and mapping, and the base itself is
                // re-measured on its pixels the next time it is decoded.
                let marked = mark_inherited_film_base(&pipeline_state)?;
                transaction
                    .execute(
                        "UPDATE image_states
                         SET base_color = ?1, pipeline_state = ?2, updated_at = ?3
                         WHERE roll_id = ?4 AND file_path = ?5",
                        params![
                            base_color,
                            marked,
                            updated_at,
                            target.roll_id,
                            target.file_path
                        ],
                    )
                    .map_err(|error| {
                        format!(
                            "Failed to update film base for {}/{}: {error}",
                            target.roll_id, target.file_path
                        )
                    })?;
                inherited_targets.push(target.clone());
            }
        }
    }

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit settings transaction: {error}"))?;

    Ok(BatchCopyCommit {
        result: BatchCopyResult {
            updated: targets.len(),
            targets,
            modules: modules.to_vec(),
        },
        geometry: source_geometry,
        inherited_base_color: source_base_color.map(str::to_string),
        inherited_targets,
    })
}

/// Keep the target's own film base but record that the inversion it now renders
/// came from another frame, so the base is re-measured on first decode.
fn mark_inherited_film_base(pipeline_state: &str) -> Result<String, String> {
    let mut state: PipelineState = serde_json::from_str(pipeline_state)
        .map_err(|error| format!("Invalid source pipeline state: {error}"))?;
    state.processing_report.base_source = crate::pipeline::INHERITED_FILM_BASE_SOURCE.to_string();
    state.processing_report.base_confidence = "1.000".to_string();
    serde_json::to_string(&state)
        .map_err(|error| format!("Failed to serialize pipeline state: {error}"))
}

/// True when the target frame already measured the film on its own pixels.
fn target_keeps_its_own_base(base_color: Option<&str>, pipeline_state: Option<&str>) -> bool {
    let Some(base_color) =
        base_color.and_then(|value| serde_json::from_str::<BaseColor>(value).ok())
    else {
        return false;
    };
    let source = pipeline_state
        .and_then(|value| serde_json::from_str::<PipelineState>(value).ok())
        .map(|state| state.processing_report.base_source)
        .unwrap_or_default();
    crate::pipeline::base_is_frame_measurement(&source, &base_color)
}

fn validate_modules(modules: &[String]) -> Result<(), String> {
    if modules.is_empty() {
        return Err("At least one settings module is required".to_string());
    }
    if let Some(module) = modules.iter().find(|module| {
        !matches!(
            module.as_str(),
            GEOMETRY_MODULE | FILM_AREA_MODULE | FILM_BASE_MODULE
        )
    }) {
        return Err(format!("Unsupported settings module: {module}"));
    }
    Ok(())
}

fn read_json_column(
    connection: &Connection,
    key: &ImageKey,
    column: &str,
    role: &str,
) -> Result<Value, String> {
    debug_assert_eq!(column, "geom");
    let payload = connection
        .query_row(
            "SELECT geom FROM image_states WHERE roll_id = ?1 AND file_path = ?2",
            params![key.roll_id, key.file_path],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| format!("Failed to read {role} image state: {error}"))?
        .flatten()
        .ok_or_else(|| {
            format!(
                "The {role} image state does not exist: {}/{}",
                key.roll_id, key.file_path
            )
        })?;
    serde_json::from_str(&payload).map_err(|error| {
        format!(
            "The {role} geometry JSON is invalid for {}/{}: {error}",
            key.roll_id, key.file_path
        )
    })
}

/// Recursive object merge preserves forward-compatible target keys while the
/// selected source module replaces all fields it explicitly owns.
fn merge_json(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target), Value::Object(source)) => {
            for (key, source_value) in source {
                match target.get_mut(key) {
                    Some(target_value) => merge_json(target_value, source_value),
                    None => {
                        target.insert(key.clone(), source_value.clone());
                    }
                }
            }
        }
        (target, source) => *target = source.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::json;

    fn connection() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE image_states (
                    roll_id TEXT NOT NULL,
                    file_path TEXT NOT NULL,
                    geom TEXT,
                    params TEXT,
                    base_color TEXT,
                    pipeline_state TEXT,
                    rendered_thumb_base64 TEXT,
                    updated_at INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (roll_id, file_path)
                );",
            )
            .unwrap();
        connection
    }

    fn insert(connection: &Connection, key: &ImageKey, geometry: Value, rendered: &str) {
        connection
            .execute(
                "INSERT INTO image_states (roll_id, file_path, geom, rendered_thumb_base64)
                 VALUES (?1, ?2, ?3, ?4)",
                params![key.roll_id, key.file_path, geometry.to_string(), rendered],
            )
            .unwrap();
    }

    #[test]
    fn merges_geometry_without_touching_rendered_thumbnail() {
        let mut connection = connection();
        let source = ImageKey {
            roll_id: "r1".into(),
            file_path: "source.nef".into(),
        };
        let target = ImageKey {
            roll_id: "r1".into(),
            file_path: "target.nef".into(),
        };
        insert(
            &connection,
            &source,
            json!({"calibration_points": [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]], "crop_rect": {"x": 0.1}}),
            "source-render",
        );
        insert(
            &connection,
            &target,
            json!({"calibration_points": null, "crop_rect": {"x": 0.0, "future": true}, "future_root": 42}),
            "target-render",
        );

        let commit = copy_settings_transaction(
            &mut connection,
            &source,
            std::slice::from_ref(&target),
            &[GEOMETRY_MODULE.to_string()],
            123,
        )
        .unwrap();

        assert_eq!(commit.result.updated, 1);
        let (geometry, thumbnail, updated_at): (String, String, i64) = connection
            .query_row(
                "SELECT geom, rendered_thumb_base64, updated_at FROM image_states
                 WHERE roll_id = ?1 AND file_path = ?2",
                params![target.roll_id, target.file_path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let geometry: Value = serde_json::from_str(&geometry).unwrap();
        assert_eq!(geometry["calibration_points"][0], json!([0.1, 0.1]));
        assert_eq!(geometry["crop_rect"]["x"], json!(0.1));
        assert_eq!(geometry["crop_rect"]["future"], json!(true));
        assert_eq!(geometry["future_root"], json!(42));
        assert_eq!(thumbnail, "target-render");
        assert_eq!(updated_at, 123);
    }

    #[test]
    fn missing_target_rolls_back_every_target() {
        let mut connection = connection();
        let source = ImageKey {
            roll_id: "r1".into(),
            file_path: "source.nef".into(),
        };
        let first = ImageKey {
            roll_id: "r1".into(),
            file_path: "first.nef".into(),
        };
        let missing = ImageKey {
            roll_id: "r1".into(),
            file_path: "missing.nef".into(),
        };
        insert(&connection, &source, json!({"angle": 12.0}), "source");
        insert(&connection, &first, json!({"angle": 0.0}), "first");

        let error = copy_settings_transaction(
            &mut connection,
            &source,
            &[first.clone(), missing],
            &[GEOMETRY_MODULE.to_string()],
            5,
        )
        .unwrap_err();

        assert!(error.contains("target image state does not exist"));
        let geometry: String = connection
            .query_row(
                "SELECT geom FROM image_states WHERE roll_id = ?1 AND file_path = ?2",
                params![first.roll_id, first.file_path],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&geometry).unwrap()["angle"],
            json!(0.0)
        );
    }

    #[test]
    fn film_area_copy_preserves_each_targets_crop_and_orientation() {
        let mut connection = connection();
        let source = ImageKey {
            roll_id: "r1".into(),
            file_path: "source.nef".into(),
        };
        let target = ImageKey {
            roll_id: "r1".into(),
            file_path: "target.nef".into(),
        };
        insert(
            &connection,
            &source,
            json!({
                "calibration_points": [[0.2, 0.1], [0.8, 0.1], [0.8, 0.9], [0.2, 0.9]],
                "calibration_confirmed": true,
                "crop_rect": {"x": 0.2, "y": 0.2, "width": 0.6, "height": 0.6},
                "rotate_90_count": 0
            }),
            "source",
        );
        insert(
            &connection,
            &target,
            json!({
                "calibration_points": null,
                "calibration_confirmed": false,
                "crop_rect": {"x": 0.05, "y": 0.1, "width": 0.8, "height": 0.7},
                "rotate_90_count": 1,
                "flip_h": true
            }),
            "target",
        );

        copy_settings_transaction(
            &mut connection,
            &source,
            std::slice::from_ref(&target),
            &[FILM_AREA_MODULE.to_string()],
            99,
        )
        .unwrap();

        let geometry: String = connection
            .query_row(
                "SELECT geom FROM image_states WHERE roll_id = ?1 AND file_path = ?2",
                params![target.roll_id, target.file_path],
                |row| row.get(0),
            )
            .unwrap();
        let geometry: Value = serde_json::from_str(&geometry).unwrap();
        assert_eq!(geometry["calibration_points"][0], json!([0.2, 0.1]));
        assert_eq!(geometry["calibration_confirmed"], json!(true));
        assert_eq!(geometry["crop_rect"]["x"], json!(0.05));
        assert_eq!(geometry["rotate_90_count"], json!(1));
        assert_eq!(geometry["flip_h"], json!(true));
    }

    #[test]
    fn film_area_copy_accepts_an_unconfirmed_source_draft() {
        let mut connection = connection();
        let source = ImageKey {
            roll_id: "r1".into(),
            file_path: "source.nef".into(),
        };
        let target = ImageKey {
            roll_id: "r1".into(),
            file_path: "target.nef".into(),
        };
        let draft = json!([[0.15, 0.1], [0.85, 0.1], [0.85, 0.9], [0.15, 0.9]]);
        insert(
            &connection,
            &source,
            json!({"calibration_points": draft, "calibration_confirmed": false}),
            "source",
        );
        insert(
            &connection,
            &target,
            json!({"calibration_points": null, "calibration_confirmed": false}),
            "target",
        );

        copy_settings_transaction(
            &mut connection,
            &source,
            std::slice::from_ref(&target),
            &[FILM_AREA_MODULE.to_string()],
            101,
        )
        .unwrap();

        let geometry: String = connection
            .query_row(
                "SELECT geom FROM image_states WHERE roll_id = ?1 AND file_path = ?2",
                params![target.roll_id, target.file_path],
                |row| row.get(0),
            )
            .unwrap();
        let geometry: Value = serde_json::from_str(&geometry).unwrap();
        assert_eq!(geometry["calibration_points"], draft);
        assert_eq!(geometry["calibration_confirmed"], json!(false));
    }

    fn set_film_base_state(
        connection: &Connection,
        key: &ImageKey,
        params: &Value,
        base_color: &str,
        state: &PipelineState,
    ) {
        connection
            .execute(
                "UPDATE image_states
                 SET params = ?1, base_color = ?2, pipeline_state = ?3
                 WHERE roll_id = ?4 AND file_path = ?5",
                params![
                    params.to_string(),
                    base_color,
                    serde_json::to_string(state).unwrap(),
                    key.roll_id,
                    key.file_path
                ],
            )
            .unwrap();
    }

    fn film_base_state(base_source: &str) -> PipelineState {
        let mut state = PipelineState::smart_auto();
        state.processing_report.base_source = base_source.to_string();
        state.processing_report.base_confidence = "0.950".to_string();
        state
    }

    fn film_base_pair() -> (Connection, ImageKey, ImageKey) {
        let connection = connection();
        let source = ImageKey {
            roll_id: "r1".into(),
            file_path: "source.nef".into(),
        };
        let target = ImageKey {
            roll_id: "r1".into(),
            file_path: "target.nef".into(),
        };
        insert(
            &connection,
            &source,
            json!({"calibration_points": null}),
            "source",
        );
        insert(
            &connection,
            &target,
            json!({"calibration_points": null}),
            "target",
        );
        (connection, source, target)
    }

    fn target_row(
        connection: &Connection,
        target: &ImageKey,
    ) -> (String, String, String, String, i64) {
        connection
            .query_row(
                "SELECT params, base_color, pipeline_state, rendered_thumb_base64, updated_at
                 FROM image_states WHERE roll_id = ?1 AND file_path = ?2",
                params![target.roll_id, target.file_path],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap()
    }

    /// Batch Apply carries the physical settings of the frame: its Film Area
    /// (that module is covered above) and its film base. The display endpoints
    /// are what Copy Settings broadcasts, so they must not move here, and the
    /// base it installs is a place-holder the frame re-measures as soon as it
    /// has pixels of its own.
    #[test]
    fn film_base_copy_marks_the_base_and_leaves_the_endpoints() {
        let (mut connection, source, target) = film_base_pair();
        set_film_base_state(
            &connection,
            &source,
            &json!({
                "d_min": [0.12, 0.13, 0.14],
                "d_max": [1.02, 1.13, 1.24],
                "d_min_offset": -0.05,
                "d_max_offset": 0.03,
                "exposure": 0.4
            }),
            "[17338,12035,6787]",
            &film_base_state("film_edge_band"),
        );
        let mut target_state = film_base_state("unresolved");
        target_state.processing_report.tone_mapping_mode =
            "preserve_tone_adaptive_midpoint".to_string();
        set_film_base_state(
            &connection,
            &target,
            &json!({
                "d_min": [0.1, 0.1, 0.1],
                "d_max": [2.0, 2.0, 2.0],
                "exposure": 0.0
            }),
            "[32768,32768,32768]",
            &target_state,
        );

        let commit = copy_settings_transaction(
            &mut connection,
            &source,
            std::slice::from_ref(&target),
            &[FILM_BASE_MODULE.to_string()],
            202,
        )
        .unwrap();

        assert_eq!(commit.result.updated, 1);
        assert_eq!(commit.inherited_targets, vec![target.clone()]);
        let (params, base_color, pipeline, thumbnail, updated_at) =
            target_row(&connection, &target);
        let params: Value = serde_json::from_str(&params).unwrap();
        assert_eq!(
            params,
            json!({
                "d_min": [0.1, 0.1, 0.1],
                "d_max": [2.0, 2.0, 2.0],
                "exposure": 0.0
            }),
            "Batch Apply must not copy another frame's endpoints"
        );
        assert_eq!(base_color, "[17338,12035,6787]");
        let state: PipelineState = serde_json::from_str(&pipeline).unwrap();
        assert_eq!(
            state.processing_report.base_source,
            crate::pipeline::INHERITED_FILM_BASE_SOURCE
        );
        assert_eq!(state.processing_report.base_confidence, "1.000");
        assert_eq!(
            state.processing_report.tone_mapping_mode, "preserve_tone_adaptive_midpoint",
            "the target keeps its own state; only the base provenance changes"
        );
        assert_eq!(thumbnail, "target");
        assert_eq!(updated_at, 202);
    }

    /// A target that already measured the film on its own pixels keeps that
    /// measurement: only the recipe is applied to it.
    #[test]
    fn film_base_copy_keeps_a_target_that_measured_its_own_base() {
        let (mut connection, source, target) = film_base_pair();
        set_film_base_state(
            &connection,
            &source,
            &json!({"d_min": [0.12, 0.13, 0.14], "d_max": [1.02, 1.13, 1.24]}),
            "[17338,12035,6787]",
            &film_base_state("film_edge_band"),
        );
        set_film_base_state(
            &connection,
            &target,
            &json!({"d_min": [0.1, 0.1, 0.1], "d_max": [2.0, 2.0, 2.0]}),
            "[14036,11366,8402]",
            &film_base_state("film_edge_band"),
        );

        let commit = copy_settings_transaction(
            &mut connection,
            &source,
            std::slice::from_ref(&target),
            &[FILM_BASE_MODULE.to_string()],
            303,
        )
        .unwrap();

        assert!(commit.inherited_targets.is_empty());
        let (params, base_color, pipeline, _, _) = target_row(&connection, &target);
        let params: Value = serde_json::from_str(&params).unwrap();
        assert_eq!(
            params,
            json!({"d_min": [0.1, 0.1, 0.1], "d_max": [2.0, 2.0, 2.0]}),
            "Batch Apply must not copy another frame's endpoints"
        );
        assert_eq!(base_color, "[14036,11366,8402]");
        let state: PipelineState = serde_json::from_str(&pipeline).unwrap();
        assert_eq!(state.processing_report.base_source, "film_edge_band");
    }
}
