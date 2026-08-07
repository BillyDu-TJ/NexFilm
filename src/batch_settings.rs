use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

const GEOMETRY_MODULE: &str = "geometry";
const FILM_AREA_MODULE: &str = "film_area";

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
    })
}

fn validate_modules(modules: &[String]) -> Result<(), String> {
    if modules.is_empty() {
        return Err("At least one settings module is required".to_string());
    }
    if let Some(module) = modules
        .iter()
        .find(|module| !matches!(module.as_str(), GEOMETRY_MODULE | FILM_AREA_MODULE))
    {
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
}
