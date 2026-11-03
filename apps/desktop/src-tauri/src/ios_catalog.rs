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
const SCHEMA_VERSION: i64 = 5;
const MAX_AUDIO_BYTES: u64 = 1_073_741_824;

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

#[derive(Debug, Clone, PartialEq)]
pub struct ArtworkPayload {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ImportTrackResult {
    pub track: CatalogTrack,
    pub duplicate: bool,
    pub metadata_warning: Option<String>,
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
pub async fn get_track_artwork(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    track_id: i64,
) -> Result<tauri::ipc::Response, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let lock = state.0.clone();
    let artwork = tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().map_err(|_| "Library is busy".to_string())?;
        artwork_for_track_at(&root, track_id)
    })
    .await
    .map_err(|error| error.to_string())??;
    // Tauri transports raw IPC bytes as an ArrayBuffer, avoiding JSON arrays
    // (which multiply image payload size and allocation on iPhone).
    Ok(tauri::ipc::Response::new(
        artwork.map(|payload| payload.bytes).unwrap_or_default(),
    ))
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
        version = 2;
    }
    if version <= 2 {
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute_batch("CREATE TABLE spotify_imports (id INTEGER PRIMARY KEY, source_digest TEXT NOT NULL UNIQUE, row_count INTEGER NOT NULL);
          CREATE TABLE catalog_tracks (id INTEGER PRIMARY KEY, title TEXT NOT NULL, artist TEXT, album TEXT, spotify_id TEXT, isrc TEXT);
          CREATE TABLE spotify_rows (batch_id INTEGER NOT NULL REFERENCES spotify_imports(id), source_record INTEGER NOT NULL, line_start INTEGER NOT NULL, line_end INTEGER NOT NULL, source_type TEXT, playlist_label TEXT, catalog_id INTEGER REFERENCES catalog_tracks(id), entry_id INTEGER REFERENCES playlist_entries(id) ON DELETE SET NULL, source_cells TEXT NOT NULL, PRIMARY KEY(batch_id, source_record));
          ALTER TABLE playlist_entries ADD COLUMN catalog_id INTEGER REFERENCES catalog_tracks(id);
          PRAGMA user_version = 3;").map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        version = 3;
    }
    if version <= 3 {
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute_batch("CREATE TABLE audio_metadata(track_id INTEGER PRIMARY KEY REFERENCES imported_tracks(id),title TEXT,artist TEXT,album TEXT,isrc TEXT,read_error TEXT);
          CREATE TABLE audio_matches(catalog_id INTEGER PRIMARY KEY REFERENCES catalog_tracks(id),track_id INTEGER NOT NULL REFERENCES imported_tracks(id),method TEXT NOT NULL CHECK(method IN ('automatic','manual')));
          CREATE TABLE match_suppressions(catalog_id INTEGER PRIMARY KEY REFERENCES catalog_tracks(id));
          PRAGMA user_version=4;").map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        version = 4;
    }
    if version <= 4 {
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let has_local_identifier: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('catalog_tracks') WHERE name='local_identifier')",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !has_local_identifier {
            tx.execute_batch("ALTER TABLE catalog_tracks ADD COLUMN local_identifier TEXT;")
                .map_err(|e| e.to_string())?;
        }
        tx.execute_batch(
            "UPDATE catalog_tracks SET local_identifier = 'spotify-' || id WHERE local_identifier IS NULL;
             CREATE UNIQUE INDEX IF NOT EXISTS idx_catalog_tracks_local_identifier
                ON catalog_tracks(local_identifier) WHERE local_identifier IS NOT NULL;
             PRAGMA user_version=5;",
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    Ok(connection)
}

pub(crate) fn list_tracks_at(app_local_data: &Path, offset: usize) -> Result<TrackPage, String> {
    let root = app_root(app_local_data)?;
    let media_dir = ensure_child_dir(&root, MEDIA_FOLDER)?;
    let staging_dir = ensure_child_dir(&root, STAGING_FOLDER)?;
    let connection = open_catalog(&root)?;
    let report = report_orphans(&connection, &media_dir, &staging_dir)?;
    if report.staged_files > 0 || report.unreferenced_media_files > 0 {
        eprintln!(
            "MyPod retained files for review: {} staged/partial file(s), {} unreferenced managed media file(s)",
            report.staged_files, report.unreferenced_media_files
        );
    }
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
            apply_embedded_metadata(&connection, &mut track)?;
            Ok(track)
        })
        .collect::<Result<Vec<_>, String>>()?;
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
    apply_embedded_metadata(&connection, &mut track)?;
    Ok(track)
}

fn artwork_for_track_at(app_local_data: &Path, track_id: i64) -> Result<Option<ArtworkPayload>, String> {
    if track_id <= 0 {
        return Err("A valid library track ID is required".to_string());
    }
    let root = app_root(app_local_data)?;
    let connection = open_catalog(&root)?;
    let metadata = connection
        .query_row(
            "SELECT imported_tracks.id, audio_metadata.artist, audio_metadata.album \
             FROM imported_tracks LEFT JOIN audio_metadata ON audio_metadata.track_id = imported_tracks.id \
             WHERE imported_tracks.id = ?1",
            [track_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Track not found".to_string())?;

    let mut candidates = vec![track_id];
    if metadata.2.as_deref().is_some_and(|album| !album.trim().is_empty()) {
        let mut query = connection
            .prepare(
                "SELECT track_id FROM audio_metadata WHERE track_id != ?1 \
                 AND album = ?2 COLLATE NOCASE AND artist IS ?3 ORDER BY track_id LIMIT 24",
            )
            .map_err(|error| error.to_string())?;
        let peers = query
            .query_map(rusqlite::params![track_id, metadata.2, metadata.1], |row| row.get::<_, i64>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        candidates.extend(peers);
    }

    for candidate_id in candidates {
        let path = match resolve_track_audio_path_at(app_local_data, candidate_id) {
            Ok(path) => path,
            Err(_) => continue,
        };
        if let Some((bytes, mime_type)) = crate::ios_audio_matching::read_cover_art(&path) {
            return Ok(Some(ArtworkPayload { mime_type, bytes }));
        }
    }
    Ok(None)
}

fn apply_embedded_metadata(db: &Connection, track: &mut CatalogTrack) -> Result<(), String> {
    let metadata = db
        .query_row(
            "SELECT title,artist,album FROM audio_metadata WHERE track_id=?1",
            [track.id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((title, artist, album)) = metadata {
        if let Some(title) = title.filter(|v| !v.trim().is_empty()) {
            track.title = title;
        }
        track.artist = artist;
        track.album = album;
    }
    Ok(())
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
    let metadata =
        fs::metadata(&staged).map_err(|_| "The selected file could not be copied".to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_AUDIO_BYTES {
        return Err("The selected file is empty or exceeds the 1 GiB import limit".to_string());
    }
    if !matches_audio_header(&staged, &extension)? {
        return Err("The selected file does not match a supported audio format".to_string());
    }
    let audio_metadata = crate::ios_audio_matching::read_metadata(&staged);
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
        let (mut track, relative) = existing;
        transaction.commit().map_err(|error| error.to_string())?;
        if !managed_file_exists(&root, &relative) {
            let leaf = Path::new(&relative).file_name().ok_or_else(|| "Invalid catalog media path".to_string())?;
            fs::rename(&staged, media_dir.join(leaf)).map_err(|error| error.to_string())?;
            track.is_missing = 0;
        } else {
            fs::remove_file(&staged).map_err(|error| error.to_string())?;
        }
        let tx = connection.transaction().map_err(|e|e.to_string())?;
        let has_metadata: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM audio_metadata WHERE track_id=?1)", [track.id], |r|r.get(0)).map_err(|e|e.to_string())?;
        if !has_metadata { crate::ios_audio_matching::save_metadata(&tx, track.id, &audio_metadata)?; }
        crate::ios_audio_matching::auto_match(&tx, &root)?;
        tx.commit().map_err(|e|e.to_string())?;
        apply_embedded_metadata(&connection, &mut track)?;
        return Ok(ImportTrackResult { track, duplicate: true, metadata_warning: audio_metadata.error });
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
        return Err(error.to_string());
    }
    let id = transaction.last_insert_rowid();
    let matching = crate::ios_audio_matching::save_metadata(&transaction, id, &audio_metadata)
        .and_then(|_| crate::ios_audio_matching::auto_match(&transaction, &root));
    if let Err(error) = matching {
        drop(transaction);
        return Err(error);
    }
    transaction.commit().map_err(|error| error.to_string())?;
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
        metadata_warning: audio_metadata.error,
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
struct OrphanReport {
    staged_files: usize,
    unreferenced_media_files: usize,
}

fn report_orphans(
    connection: &Connection,
    media_dir: &Path,
    staging_dir: &Path,
) -> Result<OrphanReport, String> {
    report_orphans_at(connection, media_dir, staging_dir)
}

fn report_orphans_at(
    connection: &Connection,
    media_dir: &Path,
    staging_dir: &Path,
) -> Result<OrphanReport, String> {
    let mut report = OrphanReport::default();
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
        if metadata.is_file() && path.extension().is_some_and(|ext| ext == "part") {
            report.staged_files += 1;
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
            report.unreferenced_media_files += 1;
        }
    }
    Ok(report)
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

pub(crate) fn managed_file_exists(root: &Path, relative: &str) -> bool {
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

    fn synthetic_wav_with_metadata(path: &Path, title: &str, artist: &str, album: &str, artwork: Option<Vec<u8>>) {
        synthetic_wav(path);
        use lofty::{picture::{MimeType, Picture, PictureType}, tag::{Accessor, Tag, TagExt, TagType}};
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(title.to_string());
        tag.set_artist(artist.to_string());
        tag.set_album(album.to_string());
        if let Some(artwork) = artwork {
            tag.push_picture(Picture::new_unchecked(PictureType::CoverFront, Some(MimeType::Png), None, artwork));
        }
        tag.save_to_path(path, lofty::config::WriteOptions::default()).unwrap();
    }

    fn stage_synthetic_wav(base: &Path, token: &str, title: &str, artist: &str, album: &str, artwork: Option<Vec<u8>>) {
        let root = app_root(base).unwrap();
        let staging = ensure_child_dir(&root, STAGING_FOLDER).unwrap();
        synthetic_wav_with_metadata(&staging.join(format!("{token}.part")), title, artist, album, artwork);
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
    fn reads_track_art_first_then_only_exact_album_artist_artwork() {
        let base = temp_root("artwork-fallback");
        let cover = b"\x89PNG\r\n\x1a\nsynthetic-album-cover".to_vec();
        let alternate_cover = b"\x89PNG\r\n\x1a\nsynthetic-track-cover".to_vec();
        stage_synthetic_wav(&base, "11111111111111111111111111111111", "First", "Artist", "Shared Album", None);
        stage_synthetic_wav(&base, "22222222222222222222222222222222", "Second", "Artist", "Shared Album", Some(cover.clone()));
        stage_synthetic_wav(&base, "33333333333333333333333333333333", "Third", "Artist", "Shared Album", Some(alternate_cover.clone()));
        stage_synthetic_wav(&base, "44444444444444444444444444444444", "Other artist", "Different Artist", "Shared Album", None);
        let staged_cover = app_root(&base).unwrap().join("Importing/22222222222222222222222222222222.part");
        assert_eq!(crate::ios_audio_matching::read_cover_art(&staged_cover).as_ref().map(|item| &item.0), Some(&cover));
        let first = register_import_at(&base, "11111111111111111111111111111111", "first.wav", "wav").unwrap();
        let second = register_import_at(&base, "22222222222222222222222222222222", "second.wav", "wav").unwrap();
        let third = register_import_at(&base, "33333333333333333333333333333333", "third.wav", "wav").unwrap();
        let other = register_import_at(&base, "44444444444444444444444444444444", "other.wav", "wav").unwrap();
        let second_path = resolve_track_audio_path_at(&base, second.track.id).unwrap();
        assert_eq!(crate::ios_audio_matching::read_cover_art(&second_path).as_ref().map(|item| &item.0), Some(&cover));
        let root = app_root(&base).unwrap();
        let connection = open_catalog(&root).unwrap();
        let first_meta: (Option<String>, Option<String>) = connection.query_row("SELECT artist,album FROM audio_metadata WHERE track_id=?1", [first.track.id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(first_meta, (Some("Artist".into()), Some("Shared Album".into())));
        let peer_count: i64 = connection.query_row("SELECT COUNT(*) FROM audio_metadata WHERE track_id != ?1 AND album = ?2 COLLATE NOCASE AND artist IS ?3", rusqlite::params![first.track.id, first_meta.1, first_meta.0], |row| row.get(0)).unwrap();
        assert_eq!(peer_count, 2);

        let inherited = artwork_for_track_at(&base, first.track.id).unwrap().unwrap();
        assert_eq!(inherited.mime_type, "image/png");
        assert_eq!(inherited.bytes, cover);
        assert_eq!(artwork_for_track_at(&base, second.track.id).unwrap().unwrap().bytes, cover);
        assert_eq!(artwork_for_track_at(&base, third.track.id).unwrap().unwrap().bytes, alternate_cover);
        assert_eq!(artwork_for_track_at(&base, other.track.id).unwrap(), None);
        let connection = open_catalog(&app_root(&base).unwrap()).unwrap();
        connection.execute("DELETE FROM audio_metadata WHERE track_id=?1", [second.track.id]).unwrap();
        assert_eq!(artwork_for_track_at(&base, second.track.id).unwrap().unwrap().bytes, cover);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn migrates_populated_v2_catalog_to_v5_without_losing_rows() {
        let base = temp_root("catalog-migrate-v2");
        let root = app_root(&base).unwrap();
        let db = root.join(DATABASE_FILE);
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE imported_tracks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL,
                    original_name TEXT NOT NULL, extension TEXT NOT NULL,
                    relative_path TEXT NOT NULL UNIQUE, content_sha256 TEXT NOT NULL UNIQUE,
                    file_size INTEGER NOT NULL CHECK(file_size > 0), imported_at INTEGER NOT NULL
                );
                CREATE TABLE playlists (
                    id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                    created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                    source_playlist_label TEXT, source_playlist_identifier TEXT
                );
                CREATE TABLE playlist_entries (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                    position INTEGER NOT NULL, track_id INTEGER REFERENCES imported_tracks(id) ON DELETE SET NULL,
                    title_snapshot TEXT NOT NULL, artist_snapshot TEXT, album_snapshot TEXT,
                    source_type_label TEXT, source_identifier TEXT, UNIQUE(playlist_id, position)
                );
                INSERT INTO imported_tracks VALUES (1,'Keep me','keep.wav','wav','Media/keep.wav','hash',4,1);
                INSERT INTO playlists VALUES (1,'Existing playlist',1,1,'Existing playlist','existing');
                INSERT INTO playlist_entries (playlist_id,position,track_id,title_snapshot) VALUES (1,0,1,'Keep me');
                PRAGMA user_version=2;",
            )
            .unwrap();
        drop(connection);

        let migrated = open_catalog(&root).unwrap();
        let version: i64 = migrated
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5);
        for table in [
            "spotify_imports",
            "catalog_tracks",
            "spotify_rows",
            "audio_metadata",
            "audio_matches",
            "match_suppressions",
        ] {
            let found: i64 = migrated
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(found, 1, "missing migrated table {table}");
        }
        assert_eq!(
            migrated
                .query_row("SELECT COUNT(*) FROM imported_tracks", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            migrated
                .query_row("SELECT COUNT(*) FROM playlists", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            migrated
                .query_row("SELECT COUNT(*) FROM playlist_entries", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn initializes_a_fresh_catalog_at_v5_with_local_identifiers() {
        let base = temp_root("catalog-fresh-v5");
        let root = app_root(&base).unwrap();
        let connection = open_catalog(&root).unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let has_column: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('catalog_tracks') WHERE name='local_identifier'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 5);
        assert_eq!(has_column, 1);
        drop(connection);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn upgrades_a_populated_v4_catalog_and_preserves_existing_metadata() {
        let base = temp_root("catalog-v4-to-v5");
        let root = app_root(&base).unwrap();
        let database = root.join(DATABASE_FILE);
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE catalog_tracks (
                    id INTEGER PRIMARY KEY, title TEXT NOT NULL, artist TEXT,
                    album TEXT, spotify_id TEXT, isrc TEXT
                );
                INSERT INTO catalog_tracks(id,title,artist,album,spotify_id,isrc)
                    VALUES (42,'Keep this recording','Test artist','Test album','opaque-id','TEST00000001');
                PRAGMA user_version=4;",
            )
            .unwrap();
        drop(connection);

        let migrated = open_catalog(&root).unwrap();
        let row: (String, String, String, String, String, String) = migrated
            .query_row(
                "SELECT title,artist,album,spotify_id,isrc,local_identifier FROM catalog_tracks WHERE id=42",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .unwrap();
        let version: i64 = migrated
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5);
        assert_eq!(
            row,
            (
                "Keep this recording".into(),
                "Test artist".into(),
                "Test album".into(),
                "opaque-id".into(),
                "TEST00000001".into(),
                "spotify-42".into()
            )
        );
        drop(migrated);
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
    fn preserves_staged_copy_when_catalog_open_fails() {
        let base = temp_root("catalog-failure-preserve");
        let root = app_root(&base).unwrap();
        let staging = ensure_child_dir(&root, STAGING_FOLDER).unwrap();
        let token = "55555555555555555555555555555555";
        synthetic_wav(&staging.join(format!("{token}.part")));
        fs::create_dir(root.join(DATABASE_FILE)).unwrap();

        assert!(register_import_at(&base, token, "tone.wav", "wav").is_err());
        assert!(staging.join(format!("{token}.part")).is_file());
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
        assert!(staged.is_file());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn reports_interrupted_partials_and_unreferenced_media_without_deleting_them() {
        let base = temp_root("catalog-orphans");
        let root = app_root(&base).unwrap();
        let staging = ensure_child_dir(&root, STAGING_FOLDER).unwrap();
        let media = ensure_child_dir(&root, MEDIA_FOLDER).unwrap();
        let partial = staging.join("stale.part");
        fs::write(&partial, b"interrupted").unwrap();
        let orphan = media.join("unregistered.wav");
        synthetic_wav(&orphan);
        let connection = open_catalog(&root).unwrap();
        let report = report_orphans_at(&connection, &media, &staging).unwrap();
        assert_eq!(report.staged_files, 1);
        assert_eq!(report.unreferenced_media_files, 1);
        assert!(partial.exists());
        assert!(orphan.exists());
        let _page = list_tracks_at(&base, 0).unwrap();
        assert!(partial.exists());
        assert!(orphan.exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn failed_catalog_write_preserves_existing_and_newly_staged_media() {
        let base = temp_root("catalog-write-failure-preserve");
        let root = app_root(&base).unwrap();
        let staging = ensure_child_dir(&root, STAGING_FOLDER).unwrap();
        let media = ensure_child_dir(&root, MEDIA_FOLDER).unwrap();
        let existing_token = "77777777777777777777777777777777";
        synthetic_wav(&staging.join(format!("{existing_token}.part")));
        let existing = register_import_at(&base, existing_token, "existing.wav", "wav").unwrap();
        let existing_path = media.join(format!("{existing_token}.wav"));

        let failed_token = "88888888888888888888888888888888";
        let failed_stage = staging.join(format!("{failed_token}.part"));
        synthetic_wav(&failed_stage);
        let mut distinct_audio = fs::read(&failed_stage).unwrap();
        *distinct_audio.last_mut().unwrap() = 1;
        fs::write(&failed_stage, distinct_audio).unwrap();
        let connection = open_catalog(&root).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_synthetic_import BEFORE INSERT ON imported_tracks
             BEGIN SELECT RAISE(ABORT, 'synthetic catalog write failure'); END;",
            )
            .unwrap();
        drop(connection);

        assert!(register_import_at(&base, failed_token, "failed.wav", "wav")
            .unwrap_err()
            .contains("synthetic catalog write failure"));
        let failed_path = media.join(format!("{failed_token}.wav"));
        assert!(
            existing_path.is_file(),
            "previously imported media must remain"
        );
        assert!(
            failed_path.is_file(),
            "failed transaction media must remain for recovery"
        );
        assert!(
            !failed_stage.exists(),
            "the staged file was atomically moved to managed media"
        );

        let connection = open_catalog(&root).unwrap();
        let report = report_orphans_at(&connection, &media, &staging).unwrap();
        assert_eq!(report.unreferenced_media_files, 1);
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM imported_tracks", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1,
            "the failed insert must not create a catalog row"
        );
        assert_eq!(existing.track.title, "existing");
        let _ = fs::remove_dir_all(base);
    }
}
