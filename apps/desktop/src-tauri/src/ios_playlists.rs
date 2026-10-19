use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

#[cfg(target_os = "ios")]
use tauri::Manager;

use crate::ios_catalog::{app_root, open_catalog};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub track_count: usize,
    pub created_at: i64,
    pub updated_at: i64,
    pub source_playlist_label: Option<String>,
    pub source_playlist_identifier: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlaylistEntry {
    pub id: i64,
    pub playlist_id: i64,
    pub position: i64,
    pub track_id: Option<i64>,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub source_type_label: Option<String>,
    pub source_identifier: Option<String>,
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn list_playlists(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
) -> Result<Vec<Playlist>, String> {
    with_catalog(app, state, list_playlists_at).await
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn create_playlist(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    name: String,
) -> Result<Playlist, String> {
    with_catalog(app, state, move |root| create_playlist_at(root, &name)).await
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn rename_playlist(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    playlist_id: i64,
    name: String,
) -> Result<Playlist, String> {
    with_catalog(app, state, move |root| {
        rename_playlist_at(root, playlist_id, &name)
    })
    .await
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn delete_playlist(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    playlist_id: i64,
) -> Result<(), String> {
    with_catalog(app, state, move |root| {
        delete_playlist_at(root, playlist_id)
    })
    .await
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn list_playlist_entries(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    playlist_id: i64,
) -> Result<Vec<PlaylistEntry>, String> {
    with_catalog(app, state, move |root| list_entries_at(root, playlist_id)).await
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn add_track_to_playlist(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    playlist_id: i64,
    track_id: i64,
) -> Result<PlaylistEntry, String> {
    with_catalog(app, state, move |root| {
        add_track_at(root, playlist_id, track_id)
    })
    .await
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn remove_playlist_entry(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    playlist_id: i64,
    entry_id: i64,
) -> Result<(), String> {
    with_catalog(app, state, move |root| {
        remove_entry_at(root, playlist_id, entry_id)
    })
    .await
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn reorder_playlist_entries(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    playlist_id: i64,
    entry_ids: Vec<i64>,
) -> Result<Vec<PlaylistEntry>, String> {
    with_catalog(app, state, move |root| {
        reorder_entries_at(root, playlist_id, &entry_ids)
    })
    .await
}

#[cfg(target_os = "ios")]
async fn with_catalog<T, F>(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Path) -> Result<T, String> + Send + 'static,
{
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let lock = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().map_err(|_| "Library is busy".to_string())?;
        operation(&root)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn validate_id(id: i64, kind: &str) -> Result<(), String> {
    if id <= 0 {
        Err(format!("A valid {kind} ID is required"))
    } else {
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
        return Err("Playlist name must contain 1–120 printable characters".to_string());
    }
    Ok(name.to_string())
}

pub(crate) fn list_playlists_at(app_local_data: &Path) -> Result<Vec<Playlist>, String> {
    let root = app_root(app_local_data)?;
    let connection = open_catalog(&root)?;
    let mut statement = connection
        .prepare(
            "SELECT p.id, p.name,
                    (SELECT COUNT(*) FROM playlist_entries e WHERE e.playlist_id = p.id),
                    p.created_at, p.updated_at, p.source_playlist_label, p.source_playlist_identifier
             FROM playlists p ORDER BY p.updated_at DESC, p.id DESC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                track_count: usize::try_from(row.get::<_, i64>(2)?).unwrap_or(usize::MAX),
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                source_playlist_label: row.get(5)?,
                source_playlist_identifier: row.get(6)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub(crate) fn create_playlist_at(app_local_data: &Path, name: &str) -> Result<Playlist, String> {
    let name = validate_name(name)?;
    let root = app_root(app_local_data)?;
    let mut connection = open_catalog(&root)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let now = timestamp()?;
    transaction
        .execute(
            "INSERT INTO playlists (name, created_at, updated_at) VALUES (?1, ?2, ?2)",
            params![name, now],
        )
        .map_err(|error| error.to_string())?;
    let id = transaction.last_insert_rowid();
    transaction.commit().map_err(|error| error.to_string())?;
    get_playlist_at(&root, id)
}

pub(crate) fn rename_playlist_at(
    app_local_data: &Path,
    playlist_id: i64,
    name: &str,
) -> Result<Playlist, String> {
    validate_id(playlist_id, "playlist")?;
    let name = validate_name(name)?;
    let root = app_root(app_local_data)?;
    let connection = open_catalog(&root)?;
    let changed = connection
        .execute(
            "UPDATE playlists SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, timestamp()?, playlist_id],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("Playlist not found".to_string());
    }
    get_playlist_at(&root, playlist_id)
}

fn get_playlist_at(root: &Path, playlist_id: i64) -> Result<Playlist, String> {
    let connection = open_catalog(root)?;
    connection
        .query_row(
            "SELECT p.id, p.name,
                    (SELECT COUNT(*) FROM playlist_entries e WHERE e.playlist_id = p.id),
                    p.created_at, p.updated_at, p.source_playlist_label, p.source_playlist_identifier
             FROM playlists p WHERE p.id = ?1",
            [playlist_id],
            |row| {
                Ok(Playlist {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    track_count: usize::try_from(row.get::<_, i64>(2)?).unwrap_or(usize::MAX),
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    source_playlist_label: row.get(5)?,
                    source_playlist_identifier: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Playlist not found".to_string())
}

pub(crate) fn delete_playlist_at(app_local_data: &Path, playlist_id: i64) -> Result<(), String> {
    validate_id(playlist_id, "playlist")?;
    let root = app_root(app_local_data)?;
    let connection = open_catalog(&root)?;
    let changed = connection
        .execute("DELETE FROM playlists WHERE id = ?1", [playlist_id])
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("Playlist not found".to_string());
    }
    Ok(())
}

pub(crate) fn list_entries_at(
    app_local_data: &Path,
    playlist_id: i64,
) -> Result<Vec<PlaylistEntry>, String> {
    validate_id(playlist_id, "playlist")?;
    let root = app_root(app_local_data)?;
    let connection = open_catalog(&root)?;
    ensure_playlist(&connection, playlist_id)?;
    let mut statement = connection
        .prepare(
            "SELECT e.id, e.playlist_id, e.position, e.track_id,
                    COALESCE(t.title, e.title_snapshot),
                    e.artist_snapshot,
                    e.album_snapshot,
                    e.source_type_label, e.source_identifier
             FROM playlist_entries e LEFT JOIN imported_tracks t ON t.id = e.track_id
             WHERE e.playlist_id = ?1 ORDER BY e.position",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([playlist_id], row_to_entry)
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlaylistEntry> {
    Ok(PlaylistEntry {
        id: row.get(0)?,
        playlist_id: row.get(1)?,
        position: row.get(2)?,
        track_id: row.get(3)?,
        title: row.get(4)?,
        artist: row.get(5)?,
        album: row.get(6)?,
        source_type_label: row.get(7)?,
        source_identifier: row.get(8)?,
    })
}

pub(crate) fn add_track_at(
    app_local_data: &Path,
    playlist_id: i64,
    track_id: i64,
) -> Result<PlaylistEntry, String> {
    validate_id(playlist_id, "playlist")?;
    validate_id(track_id, "track")?;
    let root = app_root(app_local_data)?;
    let mut connection = open_catalog(&root)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let title: String = transaction
        .query_row(
            "SELECT title FROM imported_tracks WHERE id = ?1",
            [track_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Track not found".to_string())?;
    ensure_playlist(&transaction, playlist_id)?;
    let position: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_entries WHERE playlist_id = ?1",
            [playlist_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO playlist_entries (playlist_id, position, track_id, title_snapshot)
             VALUES (?1, ?2, ?3, ?4)",
            params![playlist_id, position, track_id, title],
        )
        .map_err(|error| error.to_string())?;
    let id = transaction.last_insert_rowid();
    transaction
        .execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            params![timestamp()?, playlist_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    get_entry_at(&root, playlist_id, id)
}

fn get_entry_at(root: &Path, playlist_id: i64, entry_id: i64) -> Result<PlaylistEntry, String> {
    let connection = open_catalog(root)?;
    connection
        .query_row(
            "SELECT e.id, e.playlist_id, e.position, e.track_id,
                    COALESCE(t.title, e.title_snapshot), e.artist_snapshot,
                    e.album_snapshot, e.source_type_label, e.source_identifier
             FROM playlist_entries e LEFT JOIN imported_tracks t ON t.id = e.track_id
             WHERE e.playlist_id = ?1 AND e.id = ?2",
            params![playlist_id, entry_id],
            row_to_entry,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Playlist entry not found".to_string())
}

pub(crate) fn remove_entry_at(
    app_local_data: &Path,
    playlist_id: i64,
    entry_id: i64,
) -> Result<(), String> {
    validate_id(playlist_id, "playlist")?;
    validate_id(entry_id, "playlist entry")?;
    let root = app_root(app_local_data)?;
    let mut connection = open_catalog(&root)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    ensure_playlist(&transaction, playlist_id)?;
    let position: i64 = transaction
        .query_row(
            "SELECT position FROM playlist_entries WHERE id = ?1 AND playlist_id = ?2",
            params![entry_id, playlist_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Playlist entry not found".to_string())?;
    transaction
        .execute("DELETE FROM playlist_entries WHERE id = ?1", [entry_id])
        .map_err(|error| error.to_string())?;
    compact_from(&transaction, playlist_id, position)?;
    transaction
        .execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            params![timestamp()?, playlist_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

pub(crate) fn reorder_entries_at(
    app_local_data: &Path,
    playlist_id: i64,
    entry_ids: &[i64],
) -> Result<Vec<PlaylistEntry>, String> {
    validate_id(playlist_id, "playlist")?;
    let root = app_root(app_local_data)?;
    let mut connection = open_catalog(&root)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    ensure_playlist(&transaction, playlist_id)?;
    if entry_ids.iter().any(|id| *id <= 0)
        || entry_ids.iter().copied().collect::<HashSet<_>>().len() != entry_ids.len()
    {
        return Err("Playlist order must contain unique, valid entry IDs".to_string());
    }
    let existing = entry_ids_for(&transaction, playlist_id)?;
    if existing.len() != entry_ids.len() || existing.iter().any(|id| !entry_ids.contains(id)) {
        return Err("Playlist order must include every current entry exactly once".to_string());
    }
    let max_position = existing.len() as i64;
    let offset = max_position
        .checked_add(1)
        .ok_or_else(|| "Playlist is too large to reorder".to_string())?;
    transaction
        .execute(
            "UPDATE playlist_entries SET position = position + ?1 WHERE playlist_id = ?2",
            params![offset, playlist_id],
        )
        .map_err(|error| error.to_string())?;
    for (position, id) in entry_ids.iter().enumerate() {
        transaction
            .execute(
                "UPDATE playlist_entries SET position = ?1 WHERE playlist_id = ?2 AND id = ?3",
                params![position as i64, playlist_id, id],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction
        .execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            params![timestamp()?, playlist_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    list_entries_at(app_local_data, playlist_id)
}

fn compact_from(
    transaction: &rusqlite::Transaction<'_>,
    playlist_id: i64,
    start: i64,
) -> Result<(), String> {
    let mut statement = transaction
        .prepare("SELECT id FROM playlist_entries WHERE playlist_id = ?1 AND position > ?2 ORDER BY position")
        .map_err(|error| error.to_string())?;
    let ids = statement
        .query_map(params![playlist_id, start], |row| row.get::<_, i64>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    let offset = ids.len() as i64 + 1;
    transaction
        .execute(
            "UPDATE playlist_entries SET position = position + ?1 WHERE playlist_id = ?2 AND position > ?3",
            params![offset, playlist_id, start],
        )
        .map_err(|error| error.to_string())?;
    for (index, id) in ids.iter().enumerate() {
        transaction
            .execute(
                "UPDATE playlist_entries SET position = ?1 WHERE id = ?2",
                params![start + index as i64, id],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn entry_ids_for(connection: &rusqlite::Connection, playlist_id: i64) -> Result<Vec<i64>, String> {
    let mut statement = connection
        .prepare("SELECT id FROM playlist_entries WHERE playlist_id = ?1 ORDER BY position")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([playlist_id], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn ensure_playlist(connection: &rusqlite::Connection, playlist_id: i64) -> Result<(), String> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ?1)",
            [playlist_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if exists {
        Ok(())
    } else {
        Err("Playlist not found".to_string())
    }
}

fn timestamp() -> Result<i64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())
        .and_then(|value| i64::try_from(value.as_secs()).map_err(|error| error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ios_catalog::{list_tracks_at, register_import_at};
    use std::fs;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "mypod-playlist-{label}-{}-{}",
            std::process::id(),
            timestamp().unwrap()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn synthetic_wav(path: &Path) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&40_u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&8_000_u32.to_le_bytes());
        bytes.extend_from_slice(&8_000_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&8_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        fs::write(path, bytes).unwrap();
    }

    fn imported_track(base: &Path, token: &str, name: &str) -> i64 {
        let root = app_root(base).unwrap();
        let staging = root.join("Importing");
        fs::create_dir_all(&staging).unwrap();
        synthetic_wav(&staging.join(format!("{token}.part")));
        register_import_at(base, token, name, "wav")
            .unwrap()
            .track
            .id
    }

    #[test]
    fn migrates_disposable_v1_catalog_without_losing_track_or_audio() {
        let base = temp_root("migration-v1");
        let root = app_root(&base).unwrap();
        let media = root.join("Media");
        fs::create_dir_all(&media).unwrap();
        let token = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let audio = media.join(format!("{token}.wav"));
        synthetic_wav(&audio);
        let content_hash = super::super::ios_catalog::sha256_file(&audio).unwrap();
        let connection = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
        connection.execute_batch("CREATE TABLE imported_tracks (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, original_name TEXT NOT NULL, extension TEXT NOT NULL, relative_path TEXT NOT NULL UNIQUE, content_sha256 TEXT NOT NULL UNIQUE, file_size INTEGER NOT NULL CHECK(file_size > 0), imported_at INTEGER NOT NULL); CREATE INDEX idx_imported_tracks_title ON imported_tracks(title COLLATE NOCASE); PRAGMA user_version = 1;").unwrap();
        connection.execute("INSERT INTO imported_tracks(title, original_name, extension, relative_path, content_sha256, file_size, imported_at) VALUES ('Synthetic legacy', 'Synthetic legacy.wav', 'wav', ?1, ?2, ?3, 1)", params![format!("Media/{token}.wav"), content_hash, fs::metadata(&audio).unwrap().len() as i64]).unwrap();
        drop(connection);
        let migrated = open_catalog(&root).unwrap();
        let version: i64 = migrated
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let foreign_keys: i64 = migrated
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);
        let count: i64 = migrated
            .query_row("SELECT count(*) FROM imported_tracks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        assert!(audio.is_file());
        drop(migrated);
        assert_eq!(
            list_tracks_at(&base, 0).unwrap().items[0].title,
            "Synthetic legacy"
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn playlists_allow_duplicate_entries_unmatched_rows_and_same_names_and_delete_safely() {
        let base = temp_root("playlist-relations");
        let track = imported_track(&base, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "Tone.wav");
        let first = create_playlist_at(&base, "Mix").unwrap();
        let second = create_playlist_at(&base, "Mix").unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(list_playlists_at(&base).unwrap().len(), 2);
        let repeated_a = add_track_at(&base, first.id, track).unwrap();
        let repeated_b = add_track_at(&base, first.id, track).unwrap();
        assert_ne!(repeated_a.id, repeated_b.id);
        assert_eq!((repeated_a.position, repeated_b.position), (0, 1));
        let root = app_root(&base).unwrap();
        let connection = open_catalog(&root).unwrap();
        connection.execute("INSERT INTO playlist_entries (playlist_id, position, track_id, title_snapshot, source_type_label, source_identifier) VALUES (?1, 2, NULL, 'Unmatched future song', 'Track', 'opaque-id')", params![first.id]).unwrap();
        drop(connection);
        let entries = list_entries_at(&base, first.id).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].track_id, None);
        assert_eq!(entries[2].title, "Unmatched future song");
        assert_eq!(entries[2].source_identifier.as_deref(), Some("opaque-id"));
        assert_eq!(
            rename_playlist_at(&base, second.id, "Renamed")
                .unwrap()
                .name,
            "Renamed"
        );
        delete_playlist_at(&base, first.id).unwrap();
        assert!(list_entries_at(&base, first.id).is_err());
        let connection = open_catalog(&root).unwrap();
        let memberships: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM playlist_entries WHERE playlist_id = ?1",
                [first.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(memberships, 0);
        drop(connection);
        assert_eq!(get_playlist_at(&root, second.id).unwrap().name, "Renamed");
        assert_eq!(list_tracks_at(&base, 0).unwrap().total, 1);
        assert!(root
            .join("Media/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.wav")
            .is_file());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn reorder_and_remove_keep_positions_dense_and_preserve_duplicates() {
        let base = temp_root("playlist-order");
        let track = imported_track(&base, "cccccccccccccccccccccccccccccccc", "Tone.wav");
        let playlist = create_playlist_at(&base, "Order").unwrap();
        let a = add_track_at(&base, playlist.id, track).unwrap();
        let b = add_track_at(&base, playlist.id, track).unwrap();
        let c = add_track_at(&base, playlist.id, track).unwrap();
        let reordered = reorder_entries_at(&base, playlist.id, &[c.id, a.id, b.id]).unwrap();
        assert_eq!(
            reordered.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            vec![c.id, a.id, b.id]
        );
        remove_entry_at(&base, playlist.id, a.id).unwrap();
        let remaining = list_entries_at(&base, playlist.id).unwrap();
        assert_eq!(
            remaining
                .iter()
                .map(|entry| entry.position)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(
            remaining
                .iter()
                .map(|entry| entry.track_id)
                .collect::<Vec<_>>(),
            vec![Some(track), Some(track)]
        );
        let _ = fs::remove_dir_all(base);
    }
}
