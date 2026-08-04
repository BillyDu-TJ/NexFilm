use crate::app_state::{BaseColor, GeometryState, Roll, TuningParams};
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DATABASE_PATH: &str = "nexfilm_user.db";
pub const MATH_VERSION: i64 = 3;
pub const RAW_DECODE_VERSION: i64 = 5;

/// Development builds intentionally keep the database beside the repository so
/// existing projects continue to open as before. Release builds use the normal
/// per-user data directory and migrate the legacy working-directory files once.
pub fn data_root() -> PathBuf {
    if cfg!(debug_assertions) {
        return PathBuf::from(".");
    }

    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library").join("Application Support"))
        .unwrap_or_else(|| PathBuf::from("."));
    #[cfg(all(unix, not(target_os = "macos")))]
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));

    root.join("NexFilm Engine")
}

pub fn data_file(name: &str) -> PathBuf {
    let root = data_root();
    let target = root.join(name);
    if let Err(error) = std::fs::create_dir_all(&root) {
        eprintln!(
            "[Persistence] failed to create data directory {}: {error}",
            root.display()
        );
    }
    if !cfg!(debug_assertions) && !target.exists() {
        let legacy = Path::new(name);
        if legacy.exists() {
            if let Err(error) = std::fs::copy(legacy, &target) {
                eprintln!(
                    "[Persistence] failed to migrate {} to {}: {error}",
                    legacy.display(),
                    target.display()
                );
            }
        }
    }
    target
}

pub fn database_path() -> PathBuf {
    data_file(DATABASE_PATH)
}

pub fn open_connection() -> rusqlite::Result<Connection> {
    let path = database_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                error.kind(),
                format!("failed to create {}: {error}", parent.display()),
            )))
        })?;
    }
    let connection = Connection::open(path)?;
    configure_connection(&connection)?;
    Ok(connection)
}

pub fn configure_connection(connection: &Connection) -> rusqlite::Result<()> {
    // Set the busy handler before asking SQLite to switch/confirm journal mode.
    // Concurrent startup, imports, and thumbnail saves can otherwise fail while
    // another connection briefly holds the schema or WAL lock.
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA wal_autocheckpoint=1000;",
    )?;
    Ok(())
}

pub fn init_schema(connection: &Connection) -> rusqlite::Result<()> {
    configure_connection(connection)?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS user_cameras (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        )",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS user_films (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        )",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS image_states (
            roll_id TEXT NOT NULL,
            file_path TEXT NOT NULL,
            thumbnail_base64 TEXT,
            embedded_thumb_base64 TEXT,
            rendered_thumb_base64 TEXT,
            params TEXT,
            geom TEXT,
            base_color TEXT,
            math_version INTEGER NOT NULL DEFAULT 3,
            raw_decode_version INTEGER NOT NULL DEFAULT 5,
            updated_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (roll_id, file_path)
        )",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS rolls (
            roll_id TEXT PRIMARY KEY,
            date TEXT NOT NULL,
            roll_format TEXT NOT NULL,
            film_stock TEXT NOT NULL,
            camera TEXT NOT NULL,
            image_paths TEXT NOT NULL,
            sort_order INTEGER NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS app_metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        [],
    )?;

    add_column_if_missing(connection, "embedded_thumb_base64", "TEXT")?;
    add_column_if_missing(connection, "rendered_thumb_base64", "TEXT")?;
    add_column_if_missing(connection, "math_version", "INTEGER NOT NULL DEFAULT 1")?;
    add_column_if_missing(
        connection,
        "raw_decode_version",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    add_column_if_missing(connection, "updated_at", "INTEGER NOT NULL DEFAULT 0")?;
    migrate_legacy_thumbnails(connection)?;
    migrate_raw_decode_settings(connection)?;
    migrate_density_contract(connection)?;
    Ok(())
}

fn image_state_columns(connection: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut statement = connection.prepare("PRAGMA table_info(image_states)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect()
}

fn add_column_if_missing(
    connection: &Connection,
    name: &str,
    declaration: &str,
) -> rusqlite::Result<()> {
    if image_state_columns(connection)?.contains(name) {
        return Ok(());
    }
    connection.execute(
        &format!("ALTER TABLE image_states ADD COLUMN {name} {declaration}"),
        [],
    )?;
    Ok(())
}

fn migrate_legacy_thumbnails(connection: &Connection) -> rusqlite::Result<()> {
    let mut statement = connection.prepare(
        "SELECT rowid, thumbnail_base64, params, geom, base_color,
                embedded_thumb_base64, rendered_thumb_base64
         FROM image_states",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
        ))
    })?;

    let mut migrations = Vec::new();
    for row in rows {
        let (row_id, legacy, params, geom, base_color, embedded, rendered) = row?;
        let Some(legacy) = legacy.filter(|value| !value.is_empty()) else {
            continue;
        };
        let edited = params
            .as_deref()
            .and_then(|value| serde_json::from_str::<TuningParams>(value).ok())
            .is_some_and(|value| value != TuningParams::default())
            || geom
                .as_deref()
                .and_then(|value| serde_json::from_str::<GeometryState>(value).ok())
                .is_some_and(|value| value != GeometryState::default())
            || base_color
                .as_deref()
                .and_then(|value| serde_json::from_str::<BaseColor>(value).ok())
                .is_some_and(|value| value != BaseColor::default());

        migrations.push((
            row_id,
            embedded
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| legacy.clone()),
            if rendered.as_deref().is_some_and(|value| !value.is_empty()) {
                rendered
            } else if edited {
                Some(legacy)
            } else {
                None
            },
        ));
    }
    drop(statement);

    for (row_id, embedded, rendered) in migrations {
        connection.execute(
            "UPDATE image_states
             SET embedded_thumb_base64 = ?1,
                 rendered_thumb_base64 = ?2
             WHERE rowid = ?3",
            rusqlite::params![embedded, rendered, row_id],
        )?;
    }
    Ok(())
}

fn migrate_raw_decode_settings(connection: &Connection) -> rusqlite::Result<()> {
    let mut statement = connection.prepare("SELECT rowid, params FROM image_states")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let mut migrations = Vec::new();
    for row in rows {
        let (row_id, Some(original)) = row? else {
            continue;
        };
        let Ok(mut params) = serde_json::from_str::<TuningParams>(&original) else {
            continue;
        };
        params.raw_decode.working_colorspace =
            crate::color_science::DENSITY_CAPTURE_WORKING_SPACE.to_string();
        let normalized = serde_json::to_string(&params)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        if normalized != original {
            migrations.push((row_id, normalized));
        }
    }
    drop(statement);

    for (row_id, params) in migrations {
        connection.execute(
            "UPDATE image_states
             SET params = ?1, updated_at = ?2
             WHERE rowid = ?3",
            rusqlite::params![params, now_timestamp(), row_id],
        )?;
    }
    Ok(())
}

/// Version 3 measures density in a fixed linear-sRGB capture domain and keeps
/// the final positive as a display-referred sRGB signal. Base estimates and
/// rendered thumbnails generated under the previous contract cannot be reused
/// safely, so force an explicit Auto Invert while preserving user tuning and
/// geometry edits.
fn migrate_density_contract(connection: &Connection) -> rusqlite::Result<()> {
    let mut statement = connection.prepare(
        "SELECT rowid, math_version, raw_decode_version FROM image_states
         WHERE math_version < ?1 OR raw_decode_version < ?2",
    )?;
    let rows = statement.query_map(rusqlite::params![MATH_VERSION, RAW_DECODE_VERSION], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let row_ids = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);

    let default_base = serde_json::to_string(&BaseColor::default())
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    for (row_id, _, _) in row_ids {
        connection.execute(
            "UPDATE image_states
             SET base_color = ?1,
                 rendered_thumb_base64 = NULL,
                 math_version = ?2,
                 raw_decode_version = ?3,
                 updated_at = ?4
             WHERE rowid = ?5",
            rusqlite::params![
                default_base,
                MATH_VERSION,
                RAW_DECODE_VERSION,
                now_timestamp(),
                row_id
            ],
        )?;
    }
    Ok(())
}

pub fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

pub fn row_exists(
    connection: &Connection,
    roll_id: &str,
    file_path: &str,
) -> rusqlite::Result<bool> {
    connection
        .query_row(
            "SELECT 1 FROM image_states WHERE roll_id = ?1 AND file_path = ?2",
            rusqlite::params![roll_id, file_path],
            |_| Ok(true),
        )
        .optional()
        .map(|value| value.unwrap_or(false))
}

pub fn relocate_image_state(
    connection: &Connection,
    roll_id: &str,
    old_path: &str,
    new_path: &str,
) -> rusqlite::Result<usize> {
    connection.execute(
        "UPDATE image_states SET file_path = ?1, updated_at = ?2
         WHERE roll_id = ?3 AND file_path = ?4",
        rusqlite::params![new_path, now_timestamp(), roll_id, old_path],
    )
}

fn insert_roll(connection: &Connection, roll: &Roll, sort_order: usize) -> rusqlite::Result<()> {
    let image_paths = serde_json::to_string(&roll.image_paths)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    connection.execute(
        "INSERT INTO rolls (
             roll_id, date, roll_format, film_stock, camera,
             image_paths, sort_order, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            roll.roll_id,
            roll.date,
            roll.format,
            roll.film_stock,
            roll.camera,
            image_paths,
            sort_order as i64,
            now_timestamp(),
        ],
    )?;
    Ok(())
}

fn replace_rolls(connection: &Connection, rolls: &[Roll]) -> rusqlite::Result<()> {
    connection.execute("DELETE FROM rolls", [])?;
    for (index, roll) in rolls.iter().enumerate() {
        insert_roll(connection, roll, index)?;
    }
    Ok(())
}

pub fn save_rolls(connection: &mut Connection, rolls: &[Roll]) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    replace_rolls(&transaction, rolls)?;
    transaction.commit()
}

pub fn load_rolls(connection: &Connection) -> rusqlite::Result<Vec<Roll>> {
    let mut statement = connection.prepare(
        "SELECT roll_id, date, roll_format, film_stock, camera, image_paths
         FROM rolls ORDER BY sort_order, roll_id",
    )?;
    let rows = statement.query_map([], |row| {
        let image_paths_json: String = row.get(5)?;
        let image_paths = serde_json::from_str(&image_paths_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
        Ok(Roll {
            roll_id: row.get(0)?,
            date: row.get(1)?,
            format: row.get(2)?,
            film_stock: row.get(3)?,
            camera: row.get(4)?,
            image_paths,
        })
    })?;
    rows.collect()
}

pub fn migrate_legacy_rolls_if_empty(
    connection: &mut Connection,
    legacy_rolls: &[Roll],
) -> rusqlite::Result<bool> {
    let already_migrated = connection
        .query_row(
            "SELECT 1 FROM app_metadata WHERE key = 'rolls_json_migrated'",
            [],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if already_migrated {
        return Ok(false);
    }

    let transaction = connection.transaction()?;
    let count: i64 = transaction.query_row("SELECT COUNT(*) FROM rolls", [], |row| row.get(0))?;
    let migrated = count == 0 && !legacy_rolls.is_empty();
    if migrated {
        replace_rolls(&transaction, legacy_rolls)?;
    }
    transaction.execute(
        "INSERT INTO app_metadata (key, value) VALUES ('rolls_json_migrated', '1')",
        [],
    )?;
    transaction.commit()?;
    Ok(migrated)
}

pub fn delete_rolls_and_states(
    connection: &mut Connection,
    roll_ids: &[String],
    remaining_rolls: &[Roll],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    for roll_id in roll_ids {
        transaction.execute(
            "DELETE FROM image_states WHERE roll_id = ?1",
            rusqlite::params![roll_id],
        )?;
    }
    replace_rolls(&transaction, remaining_rolls)?;
    transaction.commit()
}

pub fn delete_images_and_update_rolls(
    connection: &mut Connection,
    images: &[(String, String)],
    updated_rolls: &[Roll],
) -> rusqlite::Result<usize> {
    let transaction = connection.transaction()?;
    let mut removed_states = 0;
    for (roll_id, file_path) in images {
        removed_states += transaction.execute(
            "DELETE FROM image_states WHERE roll_id = ?1 AND file_path = ?2",
            rusqlite::params![roll_id, file_path],
        )?;
    }
    replace_rolls(&transaction, updated_rolls)?;
    transaction.commit()?;
    Ok(removed_states)
}

pub fn relocate_roll_image(
    connection: &mut Connection,
    roll_id: &str,
    old_path: &str,
    new_path: &str,
    updated_rolls: &[Roll],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    let updated = relocate_image_state(&transaction, roll_id, old_path, new_path)?;
    if updated != 1 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    replace_rolls(&transaction, updated_rolls)?;
    transaction.commit()
}

pub fn write_rolls_compatibility_mirror(rolls: &[Roll]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(rolls)
        .map_err(|error| format!("Failed to serialize rolls: {error}"))?;
    std::fs::write(data_file("rolls.json"), json)
        .map_err(|error| format!("Failed to update rolls.json compatibility mirror: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_thumbnails_without_marking_unedited_frames_rendered() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute(
                "CREATE TABLE image_states (
                    roll_id TEXT NOT NULL,
                    file_path TEXT NOT NULL,
                    thumbnail_base64 TEXT,
                    params TEXT,
                    geom TEXT,
                    base_color TEXT,
                    PRIMARY KEY (roll_id, file_path)
                )",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO image_states VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "roll-a",
                    "unedited.dng",
                    "orange",
                    serde_json::to_string(&TuningParams::default()).unwrap(),
                    serde_json::to_string(&GeometryState::default()).unwrap(),
                    serde_json::to_string(&BaseColor::default()).unwrap(),
                ],
            )
            .unwrap();
        let mut edited_params = TuningParams::default();
        edited_params.exposure.exposure = 0.5;
        connection
            .execute(
                "INSERT INTO image_states VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "roll-a",
                    "edited.dng",
                    "positive",
                    serde_json::to_string(&edited_params).unwrap(),
                    serde_json::to_string(&GeometryState::default()).unwrap(),
                    serde_json::to_string(&BaseColor::default()).unwrap(),
                ],
            )
            .unwrap();

        init_schema(&connection).unwrap();
        init_schema(&connection).unwrap();

        let unedited: (String, Option<String>) = connection
            .query_row(
                "SELECT embedded_thumb_base64, rendered_thumb_base64
                 FROM image_states WHERE file_path = 'unedited.dng'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let edited: (String, Option<String>) = connection
            .query_row(
                "SELECT embedded_thumb_base64, rendered_thumb_base64
                 FROM image_states WHERE file_path = 'edited.dng'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(unedited, ("orange".to_string(), None));
        assert_eq!(
            edited,
            ("positive".to_string(), Some("positive".to_string()))
        );
    }

    #[test]
    fn removes_unsupported_camera_profiles_and_normalizes_working_spaces() {
        let connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let mut legacy_params = serde_json::to_value(TuningParams::default()).unwrap();
        legacy_params["dcp_profile"] = serde_json::Value::String("camera.dcp".to_string());
        legacy_params["working_colorspace"] =
            serde_json::Value::String("not-a-colour-space".to_string());
        connection
            .execute(
                "INSERT INTO image_states (roll_id, file_path, params)
                 VALUES ('roll-a', 'legacy.dng', ?1)",
                rusqlite::params![legacy_params.to_string()],
            )
            .unwrap();

        init_schema(&connection).unwrap();

        let (params, decode_version): (String, i64) = connection
            .query_row(
                "SELECT params, raw_decode_version FROM image_states
                 WHERE roll_id = 'roll-a' AND file_path = 'legacy.dng'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let params: serde_json::Value = serde_json::from_str(&params).unwrap();
        assert_eq!(params["working_colorspace"], "linear-srgb");
        assert!(params.get("dcp_profile").is_none());
        assert_eq!(decode_version, RAW_DECODE_VERSION);
    }

    #[test]
    fn density_contract_migration_preserves_edits_but_requires_new_auto_invert() {
        let connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let mut params = TuningParams::default();
        params.exposure.exposure = 0.375;
        let analyzed_base = BaseColor {
            base_r: 60_000,
            base_g: 50_000,
            base_b: 40_000,
        };
        connection
            .execute(
                "INSERT INTO image_states (
                    roll_id, file_path, rendered_thumb_base64, params, geom,
                    base_color, math_version, raw_decode_version
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 2, 4)",
                rusqlite::params![
                    "roll-a",
                    "old-contract.dng",
                    "stale-positive",
                    serde_json::to_string(&params).unwrap(),
                    serde_json::to_string(&GeometryState::default()).unwrap(),
                    serde_json::to_string(&analyzed_base).unwrap(),
                ],
            )
            .unwrap();

        init_schema(&connection).unwrap();

        let (stored_params, stored_base, rendered, math_version, raw_version): (
            String,
            String,
            Option<String>,
            i64,
            i64,
        ) = connection
            .query_row(
                "SELECT params, base_color, rendered_thumb_base64,
                        math_version, raw_decode_version
                 FROM image_states WHERE file_path = 'old-contract.dng'",
                [],
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
            .unwrap();
        let stored_params: TuningParams = serde_json::from_str(&stored_params).unwrap();
        let stored_base: BaseColor = serde_json::from_str(&stored_base).unwrap();
        assert_eq!(stored_params.exposure.exposure, 0.375);
        assert_eq!(stored_base, BaseColor::default());
        assert_eq!(rendered, None);
        assert_eq!(math_version, MATH_VERSION);
        assert_eq!(raw_version, RAW_DECODE_VERSION);
    }

    #[test]
    fn relocating_a_file_preserves_its_persisted_state() {
        let connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO image_states (
                    roll_id, file_path, embedded_thumb_base64, params, geom, base_color
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "roll-a",
                    "missing.dng",
                    "thumb",
                    serde_json::to_string(&TuningParams::default()).unwrap(),
                    serde_json::to_string(&GeometryState::default()).unwrap(),
                    serde_json::to_string(&BaseColor::default()).unwrap(),
                ],
            )
            .unwrap();

        assert_eq!(
            relocate_image_state(&connection, "roll-a", "missing.dng", "found.dng").unwrap(),
            1
        );
        assert!(!row_exists(&connection, "roll-a", "missing.dng").unwrap());
        assert!(row_exists(&connection, "roll-a", "found.dng").unwrap());
    }

    fn sample_roll(id: &str, paths: &[&str]) -> Roll {
        Roll {
            roll_id: id.to_string(),
            date: "2026-07-22".to_string(),
            format: "135".to_string(),
            film_stock: "Test Film".to_string(),
            camera: "Test Camera".to_string(),
            image_paths: paths.iter().map(|path| path.to_string()).collect(),
        }
    }

    #[test]
    fn migrates_legacy_rolls_only_when_the_database_is_empty() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let legacy = vec![sample_roll("roll-a", &["a.dng", "b.dng"])];

        assert!(migrate_legacy_rolls_if_empty(&mut connection, &legacy).unwrap());
        assert!(!migrate_legacy_rolls_if_empty(
            &mut connection,
            &[sample_roll("roll-b", &["other.dng"])]
        )
        .unwrap());
        assert_eq!(load_rolls(&connection).unwrap()[0].roll_id, "roll-a");
    }

    #[test]
    fn deleting_rolls_removes_metadata_and_image_states_in_one_transaction() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let roll_a = sample_roll("roll-a", &["shared.dng"]);
        let roll_b = sample_roll("roll-b", &["shared.dng"]);
        save_rolls(&mut connection, &[roll_a, roll_b.clone()]).unwrap();
        for roll_id in ["roll-a", "roll-b"] {
            connection
                .execute(
                    "INSERT INTO image_states (roll_id, file_path) VALUES (?1, ?2)",
                    rusqlite::params![roll_id, "shared.dng"],
                )
                .unwrap();
        }

        delete_rolls_and_states(&mut connection, &["roll-a".to_string()], &[roll_b]).unwrap();

        assert!(!row_exists(&connection, "roll-a", "shared.dng").unwrap());
        assert!(row_exists(&connection, "roll-b", "shared.dng").unwrap());
        let rolls = load_rolls(&connection).unwrap();
        assert_eq!(rolls.len(), 1);
        assert_eq!(rolls[0].roll_id, "roll-b");
    }

    #[test]
    fn deleting_one_image_keeps_the_roll_and_other_image_state() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let original = sample_roll("roll-a", &["a.dng", "b.dng"]);
        save_rolls(&mut connection, std::slice::from_ref(&original)).unwrap();
        for path in ["a.dng", "b.dng"] {
            connection
                .execute(
                    "INSERT INTO image_states (roll_id, file_path) VALUES (?1, ?2)",
                    rusqlite::params!["roll-a", path],
                )
                .unwrap();
        }
        let updated = sample_roll("roll-a", &["b.dng"]);

        let removed = delete_images_and_update_rolls(
            &mut connection,
            &[("roll-a".to_string(), "a.dng".to_string())],
            std::slice::from_ref(&updated),
        )
        .unwrap();

        assert_eq!(removed, 1);
        assert!(!row_exists(&connection, "roll-a", "a.dng").unwrap());
        assert!(row_exists(&connection, "roll-a", "b.dng").unwrap());
        assert_eq!(load_rolls(&connection).unwrap(), vec![updated]);
    }

    #[test]
    fn deleting_the_last_image_keeps_an_empty_roll() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let original = sample_roll("roll-a", &["only.dng"]);
        save_rolls(&mut connection, &[original]).unwrap();
        connection
            .execute(
                "INSERT INTO image_states (roll_id, file_path) VALUES (?1, ?2)",
                rusqlite::params!["roll-a", "only.dng"],
            )
            .unwrap();
        let empty_roll = sample_roll("roll-a", &[]);

        delete_images_and_update_rolls(
            &mut connection,
            &[("roll-a".to_string(), "only.dng".to_string())],
            std::slice::from_ref(&empty_roll),
        )
        .unwrap();

        assert!(!row_exists(&connection, "roll-a", "only.dng").unwrap());
        assert_eq!(load_rolls(&connection).unwrap(), vec![empty_roll]);
    }

    #[test]
    fn relocation_collision_rolls_back_image_and_roll_metadata() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let original = sample_roll("roll-a", &["old.dng", "occupied.dng"]);
        save_rolls(&mut connection, std::slice::from_ref(&original)).unwrap();
        for path in ["old.dng", "occupied.dng"] {
            connection
                .execute(
                    "INSERT INTO image_states (roll_id, file_path) VALUES (?1, ?2)",
                    rusqlite::params!["roll-a", path],
                )
                .unwrap();
        }
        let updated = sample_roll("roll-a", &["occupied.dng", "occupied.dng"]);

        assert!(relocate_roll_image(
            &mut connection,
            "roll-a",
            "old.dng",
            "occupied.dng",
            &[updated]
        )
        .is_err());

        assert!(row_exists(&connection, "roll-a", "old.dng").unwrap());
        assert_eq!(load_rolls(&connection).unwrap()[0], original);
    }
}
