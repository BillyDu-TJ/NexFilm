use crate::app_state::{
    BaseColor, CalibrationConfigProfile, CalibrationLevel, CalibrationProfilePayload,
    CalibrationReference, CalibrationReferenceKind, GeometryState, PipelineState,
    ProcessingContract, Roll, TuningParams,
};
use crate::scanner_profile::{ScannerInputProfile, ScannerProfileRecord};
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DATABASE_PATH: &str = "nexfilm_user.db";
pub const LEGACY_MATH_VERSION: i64 = 3;
pub const MATH_VERSION: i64 = 5;
pub const RAW_DECODE_VERSION: i64 = 9;
pub const LAST_USED_CALIBRATION_PROFILE_KEY: &str = "last_used_calibration_profile_id";

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
            pipeline_state TEXT NOT NULL DEFAULT '{}',
            math_version INTEGER NOT NULL DEFAULT 3,
            raw_decode_version INTEGER NOT NULL DEFAULT 6,
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
            density_anchors TEXT NOT NULL DEFAULT '{}',
            scanner_profile_id TEXT,
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
    connection.execute(
        "CREATE TABLE IF NOT EXISTS calibration_profiles (
            profile_id TEXT PRIMARY KEY,
            schema_version INTEGER NOT NULL,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            camera TEXT NOT NULL DEFAULT '',
            light_source TEXT NOT NULL DEFAULT '',
            lens TEXT NOT NULL DEFAULT '',
            calibration_level TEXT NOT NULL,
            notes TEXT NOT NULL DEFAULT '',
            payload TEXT NOT NULL DEFAULT '{}'
        )",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS calibration_references (
            reference_id TEXT PRIMARY KEY,
            profile_id TEXT NOT NULL,
            reference_kind TEXT NOT NULL,
            file_path TEXT NOT NULL,
            file_name TEXT NOT NULL,
            file_size INTEGER NOT NULL DEFAULT 0,
            modified_at INTEGER,
            added_at INTEGER NOT NULL,
            FOREIGN KEY (profile_id) REFERENCES calibration_profiles(profile_id) ON DELETE CASCADE
        )",
        [],
    )?;
    connection.execute(
        "CREATE INDEX IF NOT EXISTS calibration_references_profile_idx
         ON calibration_references(profile_id, added_at, reference_id)",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS calibration_sessions (
            session_id TEXT PRIMARY KEY,
            profile_id TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            validation_status TEXT NOT NULL,
            payload TEXT NOT NULL,
            FOREIGN KEY (profile_id) REFERENCES calibration_profiles(profile_id) ON DELETE CASCADE
        )",
        [],
    )?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS scanner_profiles (
            profile_id TEXT PRIMARY KEY,
            source_path TEXT NOT NULL,
            source_digest TEXT NOT NULL,
            payload TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT 0
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
    add_column_if_missing(connection, "pipeline_state", "TEXT NOT NULL DEFAULT '{}'")?;
    add_roll_column_if_missing(connection, "density_anchors", "TEXT NOT NULL DEFAULT '{}'")?;
    add_roll_column_if_missing(connection, "calibration_profile_id", "TEXT")?;
    add_roll_column_if_missing(connection, "scanner_profile_id", "TEXT")?;
    add_calibration_profile_column_if_missing(connection, "payload", "TEXT NOT NULL DEFAULT '{}'")?;
    migrate_legacy_thumbnails(connection)?;
    migrate_raw_decode_settings(connection)?;
    migrate_density_contract(connection)?;
    migrate_p11_calibration_contract(connection)?;
    Ok(())
}

fn migrate_p11_calibration_contract(connection: &Connection) -> rusqlite::Result<()> {
    let default_payload = serde_json::to_string(&CalibrationProfilePayload::default())
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    connection.execute(
        "UPDATE calibration_profiles
         SET schema_version = ?1, payload = ?2, calibration_level = ?3, updated_at = ?4
         WHERE schema_version = 1",
        rusqlite::params![
            crate::app_state::CALIBRATION_PROFILE_SCHEMA_VERSION as i64,
            default_payload,
            serialize_enum(&CalibrationLevel::SmartAuto)?,
            now_timestamp(),
        ],
    )?;

    for (table, key_column) in [("rolls", "roll_id"), ("image_states", "rowid")] {
        let column = if table == "rolls" {
            "density_anchors"
        } else {
            "pipeline_state"
        };
        let mut statement =
            connection.prepare(&format!("SELECT {key_column}, {column} FROM {table}"))?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, rusqlite::types::Value>(0)?,
                row.get::<_, String>(1)?,
            ))
        })?;
        let mut normalized = Vec::new();
        for row in rows {
            let (key, original) = row?;
            let value = if table == "rolls" {
                serde_json::from_str::<crate::app_state::DensityAnchors>(&original)
                    .ok()
                    .and_then(|value| serde_json::to_string(&value).ok())
            } else {
                serde_json::from_str::<PipelineState>(&original)
                    .ok()
                    .and_then(|value| serde_json::to_string(&value).ok())
            };
            if let Some(value) = value.filter(|value| value != &original) {
                normalized.push((key, value));
            }
        }
        drop(statement);
        for (key, value) in normalized {
            connection.execute(
                &format!("UPDATE {table} SET {column} = ?1 WHERE {key_column} = ?2"),
                rusqlite::params![value, key],
            )?;
        }
    }
    Ok(())
}

fn image_state_columns(connection: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut statement = connection.prepare("PRAGMA table_info(image_states)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect()
}

fn roll_columns(connection: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut statement = connection.prepare("PRAGMA table_info(rolls)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect()
}

fn calibration_profile_columns(connection: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut statement = connection.prepare("PRAGMA table_info(calibration_profiles)")?;
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

fn add_roll_column_if_missing(
    connection: &Connection,
    name: &str,
    declaration: &str,
) -> rusqlite::Result<()> {
    if roll_columns(connection)?.contains(name) {
        return Ok(());
    }
    connection.execute(
        &format!("ALTER TABLE rolls ADD COLUMN {name} {declaration}"),
        [],
    )?;
    Ok(())
}

fn add_calibration_profile_column_if_missing(
    connection: &Connection,
    name: &str,
    declaration: &str,
) -> rusqlite::Result<()> {
    if calibration_profile_columns(connection)?.contains(name) {
        return Ok(());
    }
    connection.execute(
        &format!("ALTER TABLE calibration_profiles ADD COLUMN {name} {declaration}"),
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
    let rows = statement.query_map(
        rusqlite::params![LEGACY_MATH_VERSION, RAW_DECODE_VERSION],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )?;
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
                LEGACY_MATH_VERSION,
                RAW_DECODE_VERSION,
                now_timestamp(),
                row_id
            ],
        )?;
    }
    Ok(())
}

pub fn math_version_for_contract(contract: ProcessingContract) -> i64 {
    match contract {
        ProcessingContract::LegacyV1 => LEGACY_MATH_VERSION,
        _ => MATH_VERSION,
    }
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
    let density_anchors = serde_json::to_string(&roll.density_anchors)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    connection.execute(
        "INSERT INTO rolls (
             roll_id, date, roll_format, film_stock, camera,
             image_paths, density_anchors, calibration_profile_id, scanner_profile_id, sort_order, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            roll.roll_id,
            roll.date,
            roll.format,
            roll.film_stock,
            roll.camera,
            image_paths,
            density_anchors,
            roll.calibration_profile_id,
            roll.scanner_profile_id,
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

pub fn save_rolls_and_pipeline_states(
    connection: &mut Connection,
    rolls: &[Roll],
    pipeline_states: &[(String, String, PipelineState)],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    replace_rolls(&transaction, rolls)?;
    for (roll_id, file_path, pipeline_state) in pipeline_states {
        let serialized = serde_json::to_string(pipeline_state)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        transaction.execute(
            "UPDATE image_states
             SET pipeline_state = ?1, math_version = ?2, updated_at = ?3
             WHERE roll_id = ?4 AND file_path = ?5",
            rusqlite::params![
                serialized,
                math_version_for_contract(pipeline_state.contract),
                now_timestamp(),
                roll_id,
                file_path,
            ],
        )?;
    }
    transaction.commit()
}

/// Persist a roll's resolved pipeline state and invalidate rendered thumbnails
/// when the mapping itself changes (for example after density-anchor edits).
/// The embedded import preview remains available as the undeveloped fallback.
pub fn save_rolls_and_pipeline_states_reset_thumbnails(
    connection: &mut Connection,
    rolls: &[Roll],
    pipeline_states: &[(String, String, PipelineState)],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    replace_rolls(&transaction, rolls)?;
    for (roll_id, file_path, pipeline_state) in pipeline_states {
        let serialized = serde_json::to_string(pipeline_state)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        transaction.execute(
            "UPDATE image_states
             SET pipeline_state = ?1, rendered_thumb_base64 = NULL,
                 thumbnail_base64 = embedded_thumb_base64, math_version = ?2,
                 updated_at = ?3
             WHERE roll_id = ?4 AND file_path = ?5",
            rusqlite::params![
                serialized,
                math_version_for_contract(pipeline_state.contract),
                now_timestamp(),
                roll_id,
                file_path,
            ],
        )?;
    }
    transaction.commit()
}

pub fn load_rolls(connection: &Connection) -> rusqlite::Result<Vec<Roll>> {
    let mut statement = connection.prepare(
        "SELECT roll_id, date, roll_format, film_stock, camera, image_paths, density_anchors,
                calibration_profile_id, scanner_profile_id
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
        let density_anchors_json: String = row.get(6)?;
        let density_anchors = serde_json::from_str(&density_anchors_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
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
            density_anchors,
            calibration_profile_id: row.get(7)?,
            scanner_profile_id: row.get(8)?,
        })
    })?;
    rows.collect()
}

fn serialize_enum<T: serde::Serialize>(value: &T) -> rusqlite::Result<String> {
    serde_json::to_string(value)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn deserialize_enum<T: serde::de::DeserializeOwned>(
    column: usize,
    value: String,
) -> rusqlite::Result<T> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn insert_calibration_reference(
    connection: &Connection,
    profile_id: &str,
    reference: &CalibrationReference,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO calibration_references (
             reference_id, profile_id, reference_kind, file_path, file_name,
             file_size, modified_at, added_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            reference.reference_id,
            profile_id,
            serialize_enum(&reference.kind)?,
            reference.file_path,
            reference.file_name,
            reference.file_size as i64,
            reference.modified_at,
            reference.added_at,
        ],
    )?;
    Ok(())
}

pub fn save_calibration_profile(
    connection: &mut Connection,
    profile: &CalibrationConfigProfile,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO calibration_profiles (
             profile_id, schema_version, name, created_at, updated_at, camera,
             light_source, lens, calibration_level, notes, payload
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(profile_id) DO UPDATE SET
             schema_version = excluded.schema_version,
             name = excluded.name,
             updated_at = excluded.updated_at,
             camera = excluded.camera,
             light_source = excluded.light_source,
             lens = excluded.lens,
             calibration_level = excluded.calibration_level,
             notes = excluded.notes,
             payload = excluded.payload",
        rusqlite::params![
            profile.profile_id,
            profile.schema_version as i64,
            profile.name,
            profile.created_at,
            profile.updated_at,
            profile.camera,
            profile.light_source,
            profile.lens,
            serialize_enum(&profile.calibration_level)?,
            profile.notes,
            serde_json::to_string(&profile.payload)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        ],
    )?;
    transaction.execute(
        "DELETE FROM calibration_references WHERE profile_id = ?1",
        rusqlite::params![profile.profile_id],
    )?;
    for reference in &profile.references {
        insert_calibration_reference(&transaction, &profile.profile_id, reference)?;
    }
    transaction.commit()
}

pub fn save_calibration_session(
    connection: &Connection,
    session_id: &str,
    profile_id: &str,
    created_at: i64,
    validation_status: &str,
    payload: &CalibrationProfilePayload,
) -> rusqlite::Result<()> {
    let payload = serde_json::to_string(payload)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    connection.execute(
        "INSERT INTO calibration_sessions (
             session_id, profile_id, created_at, validation_status, payload
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            session_id,
            profile_id,
            created_at,
            validation_status,
            payload
        ],
    )?;
    Ok(())
}

/// Atomically replaces a Profile and records the validating Session. This is
/// used after a calibration run so a digest-bearing Profile can never be left
/// without its matching passed Session (or vice versa).
pub fn save_calibration_profile_and_session(
    connection: &mut Connection,
    profile: &CalibrationConfigProfile,
    session_id: &str,
    created_at: i64,
    validation_status: &str,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO calibration_profiles (
             profile_id, schema_version, name, created_at, updated_at, camera,
             light_source, lens, calibration_level, notes, payload
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(profile_id) DO UPDATE SET
             schema_version = excluded.schema_version,
             name = excluded.name,
             updated_at = excluded.updated_at,
             camera = excluded.camera,
             light_source = excluded.light_source,
             lens = excluded.lens,
             calibration_level = excluded.calibration_level,
             notes = excluded.notes,
             payload = excluded.payload",
        rusqlite::params![
            profile.profile_id,
            profile.schema_version as i64,
            profile.name,
            profile.created_at,
            profile.updated_at,
            profile.camera,
            profile.light_source,
            profile.lens,
            serialize_enum(&profile.calibration_level)?,
            profile.notes,
            serde_json::to_string(&profile.payload)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        ],
    )?;
    transaction.execute(
        "DELETE FROM calibration_references WHERE profile_id = ?1",
        rusqlite::params![profile.profile_id],
    )?;
    for reference in &profile.references {
        insert_calibration_reference(&transaction, &profile.profile_id, reference)?;
    }
    let session_payload = serde_json::to_string(&profile.payload)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.execute(
        "INSERT INTO calibration_sessions (
             session_id, profile_id, created_at, validation_status, payload
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            session_id,
            profile.profile_id,
            created_at,
            validation_status,
            session_payload,
        ],
    )?;
    transaction.commit()
}

pub fn save_scanner_profile(
    connection: &mut Connection,
    record: &ScannerProfileRecord,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    let profile = record.profile.clone();
    if profile.profile_id.trim().is_empty() {
        return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "scanner profile id missing",
            ),
        )));
    }
    let payload = serde_json::to_string(&profile)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.execute(
        "INSERT INTO scanner_profiles (profile_id, source_path, source_digest, payload, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(profile_id) DO UPDATE SET source_path=excluded.source_path,
         source_digest=excluded.source_digest, payload=excluded.payload, updated_at=excluded.updated_at",
        rusqlite::params![profile.profile_id, record.source_path, record.source_digest, payload, now_timestamp()],
    )?;
    transaction.commit()
}

pub fn load_scanner_profiles(
    connection: &Connection,
) -> rusqlite::Result<Vec<ScannerProfileRecord>> {
    let mut statement = connection.prepare(
        "SELECT profile_id, source_path, source_digest, payload FROM scanner_profiles ORDER BY updated_at DESC, profile_id",
    )?;
    let rows = statement.query_map([], |row| {
        let profile_id: String = row.get(0)?;
        let source_path: String = row.get(1)?;
        let source_digest: String = row.get(2)?;
        let mut profile: ScannerInputProfile = serde_json::from_str(&row.get::<_, String>(3)?)
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
        profile.profile_id = profile_id;
        Ok(ScannerProfileRecord {
            profile,
            source_digest,
            source_path,
        })
    })?;
    rows.collect()
}

pub fn calibration_session_matches(
    connection: &Connection,
    session_id: &str,
    profile_id: &str,
    payload_digest: &str,
) -> rusqlite::Result<bool> {
    let payload = connection
        .query_row(
            "SELECT payload FROM calibration_sessions
             WHERE session_id = ?1 AND profile_id = ?2 AND validation_status = 'passed'",
            rusqlite::params![session_id, profile_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(payload
        .and_then(|payload| serde_json::from_str::<CalibrationProfilePayload>(&payload).ok())
        .is_some_and(|payload| {
            payload.payload_digest == payload_digest
                && payload.canonical_digest().ok().as_deref() == Some(payload_digest)
                && payload.capture_is_verified(RAW_DECODE_VERSION)
        }))
}

fn load_calibration_references(
    connection: &Connection,
    profile_id: &str,
) -> rusqlite::Result<Vec<CalibrationReference>> {
    let mut statement = connection.prepare(
        "SELECT reference_id, reference_kind, file_path, file_name, file_size, modified_at, added_at
         FROM calibration_references WHERE profile_id = ?1 ORDER BY added_at, reference_id",
    )?;
    let rows = statement.query_map(rusqlite::params![profile_id], |row| {
        let kind = deserialize_enum::<CalibrationReferenceKind>(1, row.get(1)?)
            .unwrap_or(CalibrationReferenceKind::Unknown);
        let file_size: i64 = row.get(4)?;
        Ok(CalibrationReference {
            reference_id: row.get(0)?,
            kind,
            file_path: row.get(2)?,
            file_name: row.get(3)?,
            file_size: file_size.max(0) as u64,
            modified_at: row.get(5)?,
            added_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

pub fn load_calibration_profiles(
    connection: &Connection,
) -> rusqlite::Result<Vec<CalibrationConfigProfile>> {
    let mut statement = connection.prepare(
        "SELECT profile_id, schema_version, name, created_at, updated_at, camera,
                light_source, lens, calibration_level, notes, payload
         FROM calibration_profiles ORDER BY updated_at DESC, name, profile_id",
    )?;
    let rows = statement.query_map([], |row| {
        let schema_version: i64 = row.get(1)?;
        let calibration_level = deserialize_enum::<CalibrationLevel>(8, row.get(8)?)
            .unwrap_or(CalibrationLevel::SmartAuto);
        let schema_version = if calibration_level == CalibrationLevel::SmartAuto {
            // A literal Smart Auto row is valid. An invalid level is detected
            // below from its serialized spelling so it can be isolated rather
            // than failing the entire Profile library.
            let raw_level: String = row.get(8)?;
            if raw_level == "\"smart_auto\"" {
                schema_version.max(0) as u32
            } else if serde_json::from_str::<CalibrationLevel>(&raw_level).is_err() {
                u32::MAX
            } else {
                schema_version.max(0) as u32
            }
        } else {
            schema_version.max(0) as u32
        };
        let raw_payload: String = row.get(10)?;
        let payload = serde_json::from_str::<CalibrationProfilePayload>(&raw_payload)
            .unwrap_or_else(|_| {
                let mut payload = CalibrationProfilePayload::default();
                payload.payload_version = u32::MAX;
                payload
            });
        Ok(CalibrationConfigProfile {
            profile_id: row.get(0)?,
            schema_version,
            name: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            camera: row.get(5)?,
            light_source: row.get(6)?,
            lens: row.get(7)?,
            calibration_level,
            notes: row.get(9)?,
            references: Vec::new(),
            payload,
        })
    })?;
    let mut profiles = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    for profile in &mut profiles {
        profile.references = load_calibration_references(connection, &profile.profile_id)?;
    }
    Ok(profiles)
}

pub fn delete_calibration_profile(
    connection: &Connection,
    profile_id: &str,
) -> rusqlite::Result<bool> {
    let deleted = connection.execute(
        "DELETE FROM calibration_profiles WHERE profile_id = ?1",
        rusqlite::params![profile_id],
    )? > 0;
    if deleted {
        connection.execute(
            "UPDATE app_metadata SET value = '' WHERE key = ?1 AND value = ?2",
            rusqlite::params![LAST_USED_CALIBRATION_PROFILE_KEY, profile_id],
        )?;
    }
    Ok(deleted)
}

pub fn get_last_used_calibration_profile(
    connection: &Connection,
) -> rusqlite::Result<Option<String>> {
    let value = connection
        .query_row(
            "SELECT value FROM app_metadata WHERE key = ?1",
            rusqlite::params![LAST_USED_CALIBRATION_PROFILE_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(value.filter(|value| !value.is_empty()))
}

fn set_last_used_calibration_profile(
    connection: &Connection,
    profile_id: Option<&str>,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO app_metadata (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![LAST_USED_CALIBRATION_PROFILE_KEY, profile_id.unwrap_or("")],
    )?;
    Ok(())
}

pub fn save_rolls_and_last_used_calibration_profile(
    connection: &mut Connection,
    rolls: &[Roll],
    profile_id: Option<&str>,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    replace_rolls(&transaction, rolls)?;
    set_last_used_calibration_profile(&transaction, profile_id)?;
    transaction.commit()
}

pub fn save_rolls_profile_selection_and_pipeline_states(
    connection: &mut Connection,
    rolls: &[Roll],
    profile_id: Option<&str>,
    pipeline_states: &[(String, String, PipelineState)],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    replace_rolls(&transaction, rolls)?;
    set_last_used_calibration_profile(&transaction, profile_id)?;
    for (roll_id, file_path, pipeline_state) in pipeline_states {
        let serialized = serde_json::to_string(pipeline_state)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        transaction.execute(
            "UPDATE image_states
             SET pipeline_state = ?1, math_version = ?2, updated_at = ?3
             WHERE roll_id = ?4 AND file_path = ?5",
            rusqlite::params![
                serialized,
                math_version_for_contract(pipeline_state.contract),
                now_timestamp(),
                roll_id,
                file_path,
            ],
        )?;
    }
    transaction.commit()
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
    use base64::{engine::general_purpose, Engine as _};
    use sha2::{Digest, Sha256};

    fn verified_session_payload() -> CalibrationProfilePayload {
        let mask_bytes = [0u8];
        let mut payload = CalibrationProfilePayload {
            issuer: crate::app_state::CalibrationPayloadIssuer::BackendCalibrationSession,
            calibration_session_id: Some("session-test".to_string()),
            hardware_fingerprint: "hardware-test".to_string(),
            raw_decode_version: Some(RAW_DECODE_VERSION),
            libraw_version: "0.22-test".to_string(),
            reference_frames: vec![
                crate::app_state::CalibrationReferenceSummary {
                    reference_id: "dark".to_string(),
                    kind: CalibrationReferenceKind::DarkFrame,
                    content_digest: "dark-content".to_string(),
                    raw_metadata_digest: "dark-metadata".to_string(),
                },
                crate::app_state::CalibrationReferenceSummary {
                    reference_id: "open".to_string(),
                    kind: CalibrationReferenceKind::OpenGate,
                    content_digest: "open-content".to_string(),
                    raw_metadata_digest: "open-metadata".to_string(),
                },
            ],
            capture_parameters: Some(crate::app_state::CaptureCalibrationParameters {
                correction_algorithm: crate::app_state::CAPTURE_CORRECTION_ALGORITHM_VERSION
                    .to_string(),
                demosaic_algorithm: crate::app_state::CAPTURE_DEMOSAIC_ALGORITHM_VERSION
                    .to_string(),
                epsilon: 1.0e-6,
                light_source_id: "light-test".to_string(),
                geometry_fingerprint: "geometry-test".to_string(),
            }),
            quality_mask: Some(crate::app_state::CalibrationQualityMaskSummary {
                total_samples: 1,
                valid_samples: 1,
                mask_artifact_digest: format!("{:x}", Sha256::digest(mask_bytes)),
                ..Default::default()
            }),
            mask_artifact: Some(crate::app_state::CalibrationQualityMaskArtifact {
                encoding: "invalid_bitset_le_v1".to_string(),
                sample_count: 1,
                data_base64: general_purpose::STANDARD.encode(mask_bytes),
            }),
            valid_range: Some(crate::app_state::CalibrationValidRange {
                minimum_transmission: [1.0e-6; 3],
                maximum_transmission: [1.0 + 1.0e-6; 3],
            }),
            capabilities: vec![crate::app_state::CalibrationCapability::CaptureCorrected],
            validation_report: Some(crate::app_state::CalibrationValidationReport {
                status: crate::app_state::CalibrationValidationStatus::Passed,
                checked_at: Some(1),
                checks: vec!["synthetic".to_string()],
                warnings: Vec::new(),
            }),
            ..Default::default()
        };
        payload.payload_digest = payload.canonical_digest().unwrap();
        payload
    }

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
        assert_eq!(math_version, LEGACY_MATH_VERSION);
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
            density_anchors: Default::default(),
            calibration_profile_id: None,
            scanner_profile_id: None,
        }
    }

    #[test]
    fn density_anchor_mapping_reset_clears_rendered_thumbnails() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let roll = sample_roll("roll-a", &["frame.dng"]);
        save_rolls(&mut connection, std::slice::from_ref(&roll)).unwrap();
        connection
            .execute(
                "INSERT INTO image_states
                 (roll_id, file_path, thumbnail_base64, embedded_thumb_base64, rendered_thumb_base64)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params!["roll-a", "frame.dng", "positive", "negative", "positive"],
            )
            .unwrap();

        save_rolls_and_pipeline_states_reset_thumbnails(
            &mut connection,
            std::slice::from_ref(&roll),
            &[(
                "roll-a".to_string(),
                "frame.dng".to_string(),
                PipelineState::default(),
            )],
        )
        .unwrap();

        let thumbnails: (String, Option<String>) = connection
            .query_row(
                "SELECT thumbnail_base64, rendered_thumb_base64
                 FROM image_states WHERE roll_id = 'roll-a' AND file_path = 'frame.dng'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(thumbnails, ("negative".to_string(), None));
    }

    fn sample_calibration_profile(id: &str) -> CalibrationConfigProfile {
        CalibrationConfigProfile {
            profile_id: id.to_string(),
            schema_version: crate::app_state::CALIBRATION_PROFILE_SCHEMA_VERSION,
            name: "Fixed Copy Stand".to_string(),
            created_at: 10,
            updated_at: 20,
            camera: "Test Camera".to_string(),
            light_source: "Test Light".to_string(),
            lens: "Test Lens".to_string(),
            calibration_level: CalibrationLevel::Calibrated,
            notes: "Reference set".to_string(),
            references: vec![CalibrationReference {
                reference_id: "ref-dark".to_string(),
                kind: CalibrationReferenceKind::DarkFrame,
                file_path: "dark.dng".to_string(),
                file_name: "dark.dng".to_string(),
                file_size: 1024,
                modified_at: Some(15),
                added_at: 12,
            }],
            payload: CalibrationProfilePayload::default(),
        }
    }

    #[test]
    fn legacy_roll_schema_adds_a_nullable_profile_binding() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE rolls (
                    roll_id TEXT PRIMARY KEY,
                    date TEXT NOT NULL,
                    roll_format TEXT NOT NULL,
                    film_stock TEXT NOT NULL,
                    camera TEXT NOT NULL,
                    image_paths TEXT NOT NULL,
                    density_anchors TEXT NOT NULL DEFAULT '{}',
                    sort_order INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL DEFAULT 0
                 );
                 INSERT INTO rolls VALUES (
                    'legacy-roll', '', '135', '', '', '[]', '{}', 0, 0
                 );",
            )
            .unwrap();

        init_schema(&connection).unwrap();

        let rolls = load_rolls(&connection).unwrap();
        assert_eq!(rolls.len(), 1);
        assert_eq!(rolls[0].roll_id, "legacy-roll");
        assert_eq!(rolls[0].calibration_profile_id, None);
    }

    #[test]
    fn calibration_profile_and_references_round_trip() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let profile = sample_calibration_profile("profile-a");

        save_calibration_profile(&mut connection, &profile).unwrap();

        assert_eq!(
            load_calibration_profiles(&connection).unwrap(),
            vec![profile]
        );
    }

    #[test]
    fn calibration_session_round_trip_and_tamper_invalidation() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let profile = sample_calibration_profile("session-profile");
        save_calibration_profile(&mut connection, &profile).unwrap();
        let payload = verified_session_payload();
        save_calibration_session(
            &connection,
            "session-test",
            &profile.profile_id,
            1,
            "passed",
            &payload,
        )
        .unwrap();
        assert!(calibration_session_matches(
            &connection,
            "session-test",
            &profile.profile_id,
            &payload.payload_digest
        )
        .unwrap());
        connection
            .execute(
                "UPDATE calibration_sessions SET payload = ?1 WHERE session_id = 'session-test'",
                rusqlite::params!["{}"],
            )
            .unwrap();
        assert!(!calibration_session_matches(
            &connection,
            "session-test",
            &profile.profile_id,
            &payload.payload_digest
        )
        .unwrap());
    }

    #[test]
    fn calibration_profile_and_session_commit_atomically() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let payload = verified_session_payload();
        let mut profile = sample_calibration_profile("atomic-profile");
        profile.payload = payload.clone();
        profile.calibration_level = CalibrationLevel::CaptureCorrectedExperimental;

        save_calibration_profile_and_session(
            &mut connection,
            &profile,
            "atomic-session",
            42,
            "passed",
        )
        .unwrap();
        assert!(calibration_session_matches(
            &connection,
            "atomic-session",
            &profile.profile_id,
            &payload.payload_digest
        )
        .unwrap());

        let mut changed = profile.clone();
        changed.name = "must-roll-back".to_string();
        assert!(save_calibration_profile_and_session(
            &mut connection,
            &changed,
            "atomic-session",
            43,
            "passed",
        )
        .is_err());
        assert_eq!(
            load_calibration_profiles(&connection).unwrap()[0].name,
            profile.name
        );
    }

    #[test]
    fn p11_migration_marks_legacy_profiles_and_anchors_unverified() {
        let connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let legacy_anchor = serde_json::json!({
            "density": [0.1, 0.2, 0.3],
            "source": "sampled_film_base",
            "scope": "roll",
            "confidence": "user_sampled",
            "reference_id": "legacy-anchor"
        });
        connection
            .execute(
                "INSERT INTO rolls (roll_id, date, roll_format, film_stock, camera, image_paths, density_anchors, sort_order, updated_at)
                 VALUES ('legacy-p11', '', '135', '', '', '[]', ?1, 0, 0)",
                rusqlite::params![serde_json::json!({"d_min_base": legacy_anchor}).to_string()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO calibration_profiles (profile_id, schema_version, name, created_at, updated_at, calibration_level, payload)
                 VALUES ('legacy-profile', 1, 'Legacy', 1, 1, 'calibrated', '{}')",
                [],
            )
            .unwrap();
        migrate_p11_calibration_contract(&connection).unwrap();
        let rolls = load_rolls(&connection).unwrap();
        let anchor = rolls[0].density_anchors.d_min_base.as_ref().unwrap();
        assert!(anchor.provenance.legacy);
        assert_eq!(
            anchor.provenance.input_domain,
            crate::app_state::DataDomain::ProPhotoEstimate
        );
        let profile = load_calibration_profiles(&connection)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(
            profile.payload.issuer,
            crate::app_state::CalibrationPayloadIssuer::LegacyUnverified
        );
        assert_eq!(profile.calibration_level, CalibrationLevel::SmartAuto);
    }

    #[test]
    fn roll_selection_and_last_used_are_saved_without_rebinding_older_rolls() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let mut first = sample_roll("roll-1", &["one.dng"]);
        first.calibration_profile_id = Some("profile-a".to_string());
        let mut ninth = sample_roll("roll-9", &["nine.dng"]);
        ninth.calibration_profile_id = Some("profile-b".to_string());

        save_rolls_and_last_used_calibration_profile(
            &mut connection,
            &[first.clone(), ninth.clone()],
            Some("profile-b"),
        )
        .unwrap();

        assert_eq!(load_rolls(&connection).unwrap(), vec![first, ninth]);
        assert_eq!(
            get_last_used_calibration_profile(&connection).unwrap(),
            Some("profile-b".to_string())
        );
    }

    #[test]
    fn deleting_a_profile_keeps_historical_roll_binding_but_clears_the_default() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_schema(&connection).unwrap();
        let profile = sample_calibration_profile("profile-a");
        save_calibration_profile(&mut connection, &profile).unwrap();
        let mut roll = sample_roll("roll-a", &["a.dng"]);
        roll.calibration_profile_id = Some(profile.profile_id.clone());
        save_rolls_and_last_used_calibration_profile(
            &mut connection,
            std::slice::from_ref(&roll),
            Some(&profile.profile_id),
        )
        .unwrap();

        assert!(delete_calibration_profile(&connection, &profile.profile_id).unwrap());

        assert_eq!(load_rolls(&connection).unwrap(), vec![roll]);
        assert_eq!(
            get_last_used_calibration_profile(&connection).unwrap(),
            None
        );
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
