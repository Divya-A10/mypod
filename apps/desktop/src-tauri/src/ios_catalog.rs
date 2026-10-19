use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "ios")]
use tauri::Manager;

const APP_FOLDER: &str = "MyPod";
const MEDIA_FOLDER: &str = "Media";
const STAGING_FOLDER: &str = "Importing";
const DATABASE_FILE: &str = "library.sqlite3";
const SCHEMA_VERSION: i64 = 2;
const MAX_AUDIO_BYTES: u64 = 1_073_741_824;
const STALE_IMPORT_AGE_SECONDS: u64 = 24 * 60 * 60;

struct StagedCleanup(PathBuf);

impl Drop for StagedCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CatalogTrack {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<f64>,
    pub format: String,
    pub file_size: i64,
    pub modified_at: i64,
    pub created_at: String,
    pub updated_at: String,
    pub is_missing: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ImportTrackResult {
    pub track: CatalogTrack,
    pub duplicate: bool,
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn list_tracks(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    offset: usize,
) -> Result<TrackPage, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let lock = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().map_err(|_| "Library is busy".to_string())?;
        list_tracks_at(&root, offset)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn get_track(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    track_id: i64,
) -> Result<CatalogTrack, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let lock = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().map_err(|_| "Library is busy".to_string())?;
        get_track_at(&root, track_id)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn register_imported_track(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    import_id: String,
    original_name: String,
    extension: String,
) -> Result<ImportTrackResult, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let lock = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().map_err(|_| "Library is busy".to_string())?;
        register_import_at(&root, &import_id, &original_name, &extension)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TrackPage {
    pub items: Vec<CatalogTrack>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

pub(crate) fn app_root(app_local_data: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(app_local_data).map_err(|error| error.to_string())?;
    reject_symlink(app_local_data)?;
    let root = app_local_data
        .canonicalize()
        .map_err(|error| error.to_string())?
        .join(APP_FOLDER);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    reject_symlink(&root)?;
    let canonical = root.canonicalize().map_err(|error| error.to_string())?;
    if !canonical.starts_with(
        app_local_data
            .canonicalize()
            .map_err(|error| error.to_string())?,
    ) {
        return Err("Catalog directory escaped app-private storage".to_string());
    }
    Ok(canonical)
}

fn ensure_child_dir(root: &Path, name: &str) -> Result<PathBuf, String> {
    let child = root.join(name);
    fs::create_dir_all(&child).map_err(|error| error.to_string())?;
    reject_symlink(&child)?;
    let canonical = child.canonicalize().map_err(|error| error.to_string())?;
    if !canonical.starts_with(root) {
        return Err("Media directory escaped app-private storage".to_string());
    }
    Ok(canonical)
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("Symlink paths are not accepted for imported media".to_string());
    }
    Ok(())
}

pub(crate) fn open_catalog(root: &Path) -> Result<Connection, String> {
    let database = root.join(DATABASE_FILE);
    match fs::symlink_metadata(&database) {
        Ok(_) => reject_symlink(&database)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let mut connection = Connection::open(database).map_err(|error| error.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| error.to_string())?;
    let mut version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if version > SCHEMA_VERSION {
        return Err("This MyPod catalog was created by a newer app version".to_string());
    }
    if version == 0 {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        transaction
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS imported_tracks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    original_name TEXT NOT NULL,
                    extension TEXT NOT NULL,
                    relative_path TEXT NOT NULL UNIQUE,
                    content_sha256 TEXT NOT NULL UNIQUE,
                    file_size INTEGER NOT NULL CHECK(file_size > 0),
                    imported_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_imported_tracks_title
                    ON imported_tracks(title COLLATE NOCASE);
                PRAGMA user_version = 1;",
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        version = 1;
    }
    if version == 1 {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        transaction
            .execute_batch(
                "CREATE TABLE playlists (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL CHECK(length(trim(name)) BETWEEN 1 AND 120),
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    source_playlist_label TEXT,
                    source_playlist_identifier TEXT
                );
                CREATE TABLE playlist_entries (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                    position INTEGER NOT NULL CHECK(position >= 0),
                    track_id INTEGER REFERENCES imported_tracks(id) ON DELETE SET NULL,
                    title_snapshot TEXT NOT NULL,
                    artist_snapshot TEXT,
                    album_snapshot TEXT,
                    source_type_label TEXT,
                    source_identifier TEXT,
                    UNIQUE(playlist_id, position)
                );
                CREATE INDEX idx_playlist_entries_track_id ON playlist_entries(track_id);
                CREATE INDEX idx_playlist_entries_playlist_position ON playlist_entries(playlist_id, position);
                PRAGMA user_version = 2;",
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
    }
    Ok(connection)
}

pub(crate) fn list_tracks_at(app_local_data: &Path, offset: usize) -> Result<TrackPage, String> {
    let root = app_root(app_local_data)?;
    let media_dir = ensure_child_dir(&root, MEDIA_FOLDER)?;
    let staging_dir = ensure_child_dir(&root, STAGING_FOLDER)?;
    let connection = open_catalog(&root)?;
    cleanup_orphans(&connection, &media_dir, &staging_dir)?;
    let total: i64 = connection
        .query_row("SELECT COUNT(*) FROM imported_tracks", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT id, title, original_name, extension, relative_path, content_sha256, file_size, imported_at
             FROM imported_tracks ORDER BY id DESC LIMIT 100 OFFSET ?1",
        )
        .map_err(|error| error.to_string())?;
    let items = statement
        .query_map([i64::try_from(offset).unwrap_or(i64::MAX)], row_to_record)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let items = items
        .into_iter()
        .map(|(mut track, relative_path)| {
            track.is_missing = i64::from(!managed_file_exists(&root, &relative_path));
            track
        })
        .collect();
    Ok(TrackPage {
        items,
        total: usize::try_from(total).unwrap_or(usize::MAX),
        limit: 100,
        offset,
    })
}

fn get_track_at(app_local_data: &Path, track_id: i64) -> Result<CatalogTrack, String> {
    if track_id <= 0 {
        return Err("A valid library track ID is required".to_string());
    }
    let root = app_root(app_local_data)?;
    let connection = open_catalog(&root)?;
    let (mut track, relative_path) = connection
        .query_row(
            "SELECT id, title, original_name, extension, relative_path, content_sha256, file_size, imported_at
             FROM imported_tracks WHERE id = ?1",
            [track_id],
            row_to_record,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Track not found".to_string())?;
    track.is_missing = i64::from(!managed_file_exists(&root, &relative_path));
    Ok(track)
}

pub fn resolve_track_audio_path_at(
    app_local_data: &Path,
    track_id: i64,
) -> Result<PathBuf, String> {
    if track_id <= 0 {
        return Err("A valid library track ID is required".to_string());
    }
    let root = app_root(app_local_data)?;
    let connection = open_catalog(&root)?;
    let relative: String = connection
        .query_row(
            "SELECT relative_path FROM imported_tracks WHERE id = ?1",
            [track_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Track not found".to_string())?;
    if !managed_file_exists(&root, &relative) {
        return Err("Track audio is missing or unavailable".to_string());
    }
    let media_dir = root
        .join(MEDIA_FOLDER)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let path = root
        .join(&relative)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let metadata = fs::symlink_metadata(root.join(&relative)).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || !path.starts_with(&media_dir) {
        return Err("Track audio is not a regular file in the managed media directory".to_string());
    }
    Ok(path)
}

pub(crate) fn register_import_at(
    app_local_data: &Path,
    import_id: &str,
    original_name: &str,
    extension: &str,
) -> Result<ImportTrackResult, String> {
    if !valid_import_id(import_id) {
        return Err("Invalid import token".to_string());
    }
    let extension = extension.to_ascii_lowercase();
    if !supported_extension(&extension) {
        return Err("This audio file type is not supported for import".to_string());
    }
    let safe_name = safe_display_name(original_name)?;
    let root = app_root(app_local_data)?;
    let staging_dir = ensure_child_dir(&root, STAGING_FOLDER)?;
    let media_dir = ensure_child_dir(&root, MEDIA_FOLDER)?;
    let staged = staging_dir.join(format!("{import_id}.part"));
    reject_symlink(&staged)?;
    let _staged_cleanup = StagedCleanup(staged.clone());
    let metadata =
        fs::metadata(&staged).map_err(|_| "The selected file could not be copied".to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_AUDIO_BYTES {
        return Err("The selected file is empty or exceeds the 1 GiB import limit".to_string());
    }
    if !matches_audio_header(&staged, &extension)? {
        return Err("The selected file does not match a supported audio format".to_string());
    }
    let content_hash = sha256_file(&staged)?;
    let timestamp =
        i64::try_from(now_seconds()?).map_err(|_| "System time is out of range".to_string())?;
    let title = title_from_name(&safe_name, &extension);
    let relative_path = format!("{MEDIA_FOLDER}/{import_id}.{extension}");
    let final_path = media_dir.join(format!("{import_id}.{extension}"));
    match fs::symlink_metadata(&final_path) {
        Ok(_) => {
            return Err("Import destination already exists".to_string());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }

    let mut connection = open_catalog(&root)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    if let Some(existing) = transaction
        .query_row(
            "SELECT id, title, original_name, extension, relative_path, content_sha256, file_size, imported_at
             FROM imported_tracks WHERE content_sha256 = ?1",
            [&content_hash],
            row_to_record,
        )
        .optional()
        .map_err(|error| error.to_string())?
    {
        transaction.commit().map_err(|error| error.to_string())?;
        let (mut track, relative) = existing;
        if !managed_file_exists(&root, &relative) {
            let leaf = Path::new(&relative).file_name().ok_or_else(|| "Invalid catalog media path".to_string())?;
            fs::rename(&staged, media_dir.join(leaf)).map_err(|error| error.to_string())?;
            track.is_missing = 0;
        } else {
            fs::remove_file(&staged).map_err(|error| error.to_string())?;
        }
        return Ok(ImportTrackResult { track, duplicate: true });
    }

    fs::rename(&staged, &final_path).map_err(|error| error.to_string())?;
    let insert_result = transaction.execute(
        "INSERT INTO imported_tracks
         (title, original_name, extension, relative_path, content_sha256, file_size, imported_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            title,
            safe_name,
            extension,
            relative_path,
            content_hash,
            metadata.len() as i64,
            timestamp
        ],
    );
    if let Err(error) = insert_result {
        drop(transaction);
        let _ = fs::remove_file(&final_path);
        return Err(error.to_string());
    }
    let id = transaction.last_insert_rowid();
    transaction.commit().map_err(|error| {
        let _ = fs::remove_file(&final_path);
        error.to_string()
    })?;
    Ok(ImportTrackResult {
        track: CatalogTrack {
            id,
            title,
            artist: None,
            album: None,
            duration: None,
            format: extension,
            file_size: metadata.len() as i64,
            modified_at: timestamp,
            created_at: timestamp.to_string(),
            updated_at: timestamp.to_string(),
            is_missing: 0,
        },
        duplicate: false,
    })
}

fn cleanup_orphans(
    connection: &Connection,
    media_dir: &Path,
    staging_dir: &Path,
) -> Result<(), String> {
    cleanup_orphans_at(connection, media_dir, staging_dir, now_seconds()?)
}

fn cleanup_orphans_at(
    connection: &Connection,
    media_dir: &Path,
    staging_dir: &Path,
    now: u64,
) -> Result<(), String> {
    let mut referenced = std::collections::HashSet::new();
    let mut statement = connection
        .prepare("SELECT relative_path FROM imported_tracks")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    for row in rows {
        referenced.insert(row.map_err(|error| error.to_string())?);
    }
    for entry in fs::read_dir(staging_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        reject_symlink(&path)?;
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(now);
        let age = now.saturating_sub(modified);
        if metadata.is_file()
            && path.extension().is_some_and(|ext| ext == "part")
            && age >= STALE_IMPORT_AGE_SECONDS
        {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
    }
    for entry in fs::read_dir(media_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        reject_symlink(&path)?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            continue;
        }
        let relative = format!("{MEDIA_FOLDER}/{}", entry.file_name().to_string_lossy());
        if !referenced.contains(&relative) {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<(CatalogTrack, String)> {
    let id: i64 = row.get(0)?;
    let title: String = row.get(1)?;
    let _original_name: String = row.get(2)?;
    let extension: String = row.get(3)?;
    let relative_path: String = row.get(4)?;
    let _content_hash: String = row.get(5)?;
    let file_size: i64 = row.get(6)?;
    let imported_at: i64 = row.get(7)?;
    Ok((
        CatalogTrack {
            id,
            title,
            artist: None,
            album: None,
            duration: None,
            format: extension,
            file_size,
            modified_at: imported_at,
            created_at: imported_at.to_string(),
            updated_at: imported_at.to_string(),
            is_missing: 0,
        },
        relative_path,
    ))
}

fn managed_file_exists(root: &Path, relative: &str) -> bool {
    let path = Path::new(relative);
    let mut components = path.components();
    if components
        .next()
        .is_none_or(|component| component.as_os_str() != MEDIA_FOLDER)
        || components.next().is_none()
        || components.next().is_some()
    {
        return false;
    }
    let full = root.join(path);
    reject_symlink(&full).is_ok() && full.metadata().is_ok_and(|metadata| metadata.is_file())
}

fn valid_import_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn supported_extension(extension: &str) -> bool {
    matches!(
        extension,
        "wav" | "mp3" | "m4a" | "aac" | "flac" | "ogg" | "aiff" | "aif"
    )
}

fn safe_display_name(value: &str) -> Result<String, String> {
    let basename = Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let cleaned: String = basename
        .chars()
        .filter(|character| !character.is_control())
        .take(240)
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return Err("The selected file has no usable name".to_string());
    }
    Ok(cleaned.to_string())
}

fn title_from_name(name: &str, extension: &str) -> String {
    let suffix = format!(".{extension}");
    name.strip_suffix(&suffix).unwrap_or(name).to_string()
}

fn matches_audio_header(path: &Path, extension: &str) -> Result<bool, String> {
    let mut header = [0_u8; 16];
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let read = file.read(&mut header).map_err(|error| error.to_string())?;
    let bytes = &header[..read];
    Ok(match extension {
        "wav" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "flac" => bytes.starts_with(b"fLaC"),
        "ogg" => bytes.starts_with(b"OggS"),
        "mp3" => {
            bytes.starts_with(b"ID3")
                || (bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0)
        }
        "m4a" => bytes.len() >= 12 && &bytes[4..8] == b"ftyp",
        "aac" => bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xf6 == 0xf0,
        "aiff" | "aif" => {
            bytes.len() >= 12
                && &bytes[0..4] == b"FORM"
                && (&bytes[8..12] == b"AIFF" || &bytes[8..12] == b"AIFC")
        }
        _ => false,
    })
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn now_seconds() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mypod-{label}-{}-{}",
            std::process::id(),
            now_seconds().unwrap()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn synthetic_wav(path: &Path) {
        let samples = vec![0_u8; 96];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36_u32 + samples.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&8_000_u32.to_le_bytes());
        bytes.extend_from_slice(&8_000_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&8_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&samples);
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn imports_synthetic_audio_and_reads_it_after_reopening_catalog() {
        let base = temp_root("catalog-reopen");
        let app_root = app_root(&base).unwrap();
        let staging = ensure_child_dir(&app_root, STAGING_FOLDER).unwrap();
        let token = "0123456789abcdef0123456789abcdef";
        synthetic_wav(&staging.join(format!("{token}.part")));
        let imported = register_import_at(&base, token, "Synthetic tone.wav", "wav").unwrap();
        assert!(!imported.duplicate);
        assert_eq!(imported.track.title, "Synthetic tone");
        assert!(app_root
            .join("Media/0123456789abcdef0123456789abcdef.wav")
            .is_file());
        assert_eq!(
            resolve_track_audio_path_at(&base, imported.track.id).unwrap(),
            app_root.join("Media/0123456789abcdef0123456789abcdef.wav")
        );

        drop(open_catalog(&app_root).unwrap());
        let found = get_track_at(&base, imported.track.id).unwrap();
        assert_eq!(found, imported.track);
        let listed = list_tracks_at(&base, 0).unwrap();
        assert_eq!(listed.total, 1);
        assert_eq!(listed.items, vec![imported.track]);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn audio_path_resolution_rejects_unknown_missing_and_escaped_catalog_paths() {
        let base = temp_root("catalog-audio-path");
        let root = app_root(&base).unwrap();
        let staging = ensure_child_dir(&root, STAGING_FOLDER).unwrap();
        let token = "abababababababababababababababab";
        synthetic_wav(&staging.join(format!("{token}.part")));
        let track = register_import_at(&base, token, "tone.wav", "wav")
            .unwrap()
            .track;
        assert!(resolve_track_audio_path_at(&base, 999).is_err());

        let managed = root.join("Media").join(format!("{token}.wav"));
        fs::remove_file(&managed).unwrap();
        assert!(resolve_track_audio_path_at(&base, track.id).is_err());

        let connection = open_catalog(&root).unwrap();
        connection
            .execute(
                "UPDATE imported_tracks SET relative_path = ?1 WHERE id = ?2",
                params!["../outside.wav", track.id],
            )
            .unwrap();
        assert!(resolve_track_audio_path_at(&base, track.id).is_err());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn identical_imports_are_deduplicated_and_staging_copy_removed() {
        let base = temp_root("catalog-duplicate");
        let root = app_root(&base).unwrap();
        let staging = ensure_child_dir(&root, STAGING_FOLDER).unwrap();
        let first = "11111111111111111111111111111111";
        let second = "22222222222222222222222222222222";
        synthetic_wav(&staging.join(format!("{first}.part")));
        synthetic_wav(&staging.join(format!("{second}.part")));
        let initial = register_import_at(&base, first, "tone.wav", "wav").unwrap();
        let duplicate = register_import_at(&base, second, "renamed.wav", "wav").unwrap();
        assert!(duplicate.duplicate);
        assert_eq!(duplicate.track.id, initial.track.id);
        assert!(!staging.join(format!("{second}.part")).exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn rejects_bad_tokens_invalid_audio_and_symlinked_staging_files() {
        let base = temp_root("catalog-validation");
        let root = app_root(&base).unwrap();
        let staging = ensure_child_dir(&root, STAGING_FOLDER).unwrap();
        assert!(register_import_at(&base, "../outside", "bad.wav", "wav").is_err());
        fs::write(
            staging.join("33333333333333333333333333333333.part"),
            b"not audio",
        )
        .unwrap();
        assert!(
            register_import_at(&base, "33333333333333333333333333333333", "bad.wav", "wav")
                .is_err()
        );
        #[cfg(unix)]
        {
            let external = base.join("external.wav");
            synthetic_wav(&external);
            std::os::unix::fs::symlink(
                &external,
                staging.join("44444444444444444444444444444444.part"),
            )
            .unwrap();
            assert!(register_import_at(
                &base,
                "44444444444444444444444444444444",
                "link.wav",
                "wav"
            )
            .is_err());
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn removes_staged_copy_when_catalog_open_fails() {
        let base = temp_root("catalog-failure-cleanup");
        let root = app_root(&base).unwrap();
        let staging = ensure_child_dir(&root, STAGING_FOLDER).unwrap();
        let token = "55555555555555555555555555555555";
        synthetic_wav(&staging.join(format!("{token}.part")));
        fs::create_dir(root.join(DATABASE_FILE)).unwrap();

        assert!(register_import_at(&base, token, "tone.wav", "wav").is_err());
        assert!(!staging.join(format!("{token}.part")).exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn rejects_a_file_one_byte_over_the_import_limit_before_hashing() {
        let base = temp_root("catalog-size-limit");
        let root = app_root(&base).unwrap();
        let staging = ensure_child_dir(&root, STAGING_FOLDER).unwrap();
        let token = "66666666666666666666666666666666";
        let staged = staging.join(format!("{token}.part"));
        synthetic_wav(&staged);
        File::options()
            .write(true)
            .open(&staged)
            .unwrap()
            .set_len(MAX_AUDIO_BYTES + 1)
            .unwrap();

        assert!(register_import_at(&base, token, "large.wav", "wav")
            .unwrap_err()
            .contains("1 GiB"));
        assert!(!staged.exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn cleans_stale_partial_and_unreferenced_managed_files() {
        let base = temp_root("catalog-orphans");
        let root = app_root(&base).unwrap();
        let staging = ensure_child_dir(&root, STAGING_FOLDER).unwrap();
        let media = ensure_child_dir(&root, MEDIA_FOLDER).unwrap();
        let partial = staging.join("stale.part");
        fs::write(&partial, b"interrupted").unwrap();
        let orphan = media.join("unregistered.wav");
        synthetic_wav(&orphan);
        let connection = open_catalog(&root).unwrap();
        cleanup_orphans_at(
            &connection,
            &media,
            &staging,
            now_seconds().unwrap() + STALE_IMPORT_AGE_SECONDS + 5,
        )
        .unwrap();
        assert!(!partial.exists());
        assert!(!orphan.exists());
        let _ = fs::remove_dir_all(base);
    }
}
