//! Versioned, checksummed backup bundles for the iOS-local catalog.
//!
//! The bundle is a stream, not a ZIP: `MYPODBK1`, a length-prefixed JSON
//! manifest, then the manifest's files in order. SQLite `VACUUM INTO` creates
//! a consistent database snapshot, including committed WAL content.

#[cfg(target_os = "ios")]
use crate::ios_catalog::app_root;
use crate::ios_catalog::open_catalog;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

const BACKUP_DIR: &str = "Backups";
const MEDIA_DIR: &str = "Media";
const STAGING_DIR: &str = "Importing";
const DB_FILE: &str = "library.sqlite3";
const FORMAT_VERSION: u32 = 1;
const CURRENT_SCHEMA: i64 = 5;
const MAGIC: &[u8; 8] = b"MYPODBK1";
const MAX_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;
const MAX_BACKUP_FILES: usize = 1_000_000;
const SPACE_RESERVE_BYTES: u64 = 8 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackupScope {
    DatabaseOnly,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ManifestFile {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Manifest {
    format_version: u32,
    schema_version: i64,
    journal_mode: String,
    scope: BackupScope,
    file_count: usize,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BackupSummary {
    pub format_version: u32,
    pub schema_version: i64,
    pub journal_mode: String,
    pub scope: BackupScope,
    pub file_count: usize,
    pub total_bytes: u64,
    pub sha256: String,
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn create_local_backup(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    token: String,
    scope: BackupScope,
) -> Result<BackupSummary, String> {
    use tauri::Manager;
    let data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let lock = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().map_err(|_| "Library is busy".to_string())?;
        let root = app_root(&data)?;
        create_backup_at(&root, &token, scope)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn inspect_local_backup(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    token: String,
) -> Result<BackupSummary, String> {
    use tauri::Manager;
    let data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let lock = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().map_err(|_| "Library is busy".to_string())?;
        let root = app_root(&data)?;
        inspect_backup_at(&root, &token)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn restore_local_backup(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    token: String,
    confirmed: bool,
) -> Result<BackupSummary, String> {
    use tauri::Manager;
    let data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let lock = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().map_err(|_| "Library is busy".to_string())?;
        let root = app_root(&data)?;
        restore_backup_at(&root, &token, confirmed)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) fn create_backup_at(
    root: &Path,
    token: &str,
    scope: BackupScope,
) -> Result<BackupSummary, String> {
    create_backup_with_options(root, token, scope, available_bytes, None)
}

fn create_backup_with_options(
    root: &Path,
    token: &str,
    scope: BackupScope,
    space_probe: impl Fn(&Path) -> Result<u64, String>,
    fail_after_payload_bytes: Option<u64>,
) -> Result<BackupSummary, String> {
    require_token(token)?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let backup_dir = ensure_child_dir(&root, BACKUP_DIR)?;
    let final_path = backup_dir.join(format!("{token}.mypodbackup"));
    let partial_path = backup_dir.join(format!("{token}.partial"));
    let snapshot_path = backup_dir.join(format!("{token}.snapshot.sqlite3"));
    reject_absent_destination(&final_path)?;
    reject_absent_destination(&partial_path)?;
    reject_absent_destination(&snapshot_path)?;

    let estimate = estimate_backup_bytes(&root, scope)?;
    let available = space_probe(&backup_dir)?;
    if available < estimate {
        return Err(format!(
            "Insufficient storage for this backup: need approximately {estimate} bytes, {available} bytes available"
        ));
    }

    // A backup is observational: never run migrations as a side effect. Normal
    // app startup initializes the catalog; refuse to snapshot an unexpected
    // schema and let the regular migration path handle it first.
    let source = Connection::open(root.join(DB_FILE)).map_err(|e| e.to_string())?;
    let actual_schema: i64 = source
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if actual_schema != CURRENT_SCHEMA {
        return Err(format!("Catalog schema {actual_schema} must be initialized before backup (expected {CURRENT_SCHEMA})"));
    }
    let journal_mode: String = source
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let schema_version: i64 = source
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let snapshot_sql = snapshot_path
        .to_str()
        .ok_or_else(|| "Backup path is not valid UTF-8".to_string())?;
    source
        .execute("VACUUM INTO ?1", [snapshot_sql])
        .map_err(|e| format!("Could not create a consistent SQLite snapshot: {e}"))?;
    drop(source);

    let mut sources = vec![(DB_FILE.to_string(), snapshot_path.clone())];
    if scope == BackupScope::Complete {
        verify_catalog_media_references(&root, &root.join(MEDIA_DIR), true)?;
        sources.extend(source_files(&root, MEDIA_DIR)?);
        sources.extend(source_files(&root, STAGING_DIR)?);
    }
    if sources.len() > MAX_BACKUP_FILES {
        return Err("The library has too many files for this backup format".into());
    }

    let mut files = Vec::with_capacity(sources.len());
    for (relative, path) in &sources {
        let (bytes, sha256) = file_digest(path)?;
        files.push(ManifestFile {
            path: relative.clone(),
            bytes,
            sha256,
        });
    }
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        schema_version,
        journal_mode,
        scope,
        file_count: files.len(),
        files,
    };
    validate_manifest(&manifest)?;
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
    if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Backup manifest exceeds the supported size limit".into());
    }

    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial_path)
        .map_err(|e| e.to_string())?;
    output.write_all(MAGIC).map_err(|e| e.to_string())?;
    output
        .write_all(&(manifest_bytes.len() as u64).to_le_bytes())
        .and_then(|_| output.write_all(&manifest_bytes))
        .map_err(|e| e.to_string())?;
    let mut written_payload = 0_u64;
    for ((_, source), entry) in sources.iter().zip(&manifest.files) {
        append_verified_file(
            source,
            &mut output,
            entry,
            &mut written_payload,
            fail_after_payload_bytes,
        )?;
    }
    output.sync_all().map_err(|e| e.to_string())?;
    drop(output);
    fs::rename(&partial_path, &final_path).map_err(|e| e.to_string())?;
    let (total_bytes, archive_hash) = file_digest(&final_path)?;
    let summary = summary_from_manifest(&manifest, total_bytes, archive_hash);
    // Snapshot is an operation-owned temporary copy. It is removed only after
    // the complete bundle has been flushed and atomically published.
    fs::remove_file(snapshot_path).map_err(|e| e.to_string())?;
    Ok(summary)
}

pub(crate) fn inspect_backup_at(root: &Path, token: &str) -> Result<BackupSummary, String> {
    require_token(token)?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let staging = ensure_child_dir(&root, STAGING_DIR)?;
    let archive = staging.join(format!("backup-{token}.part"));
    reject_regular_file(&archive)?;
    let manifest = validate_archive(&archive)?;
    let verify_dir = root.join(format!("BackupVerify-{token}"));
    create_fresh_directory(&verify_dir)?;
    extract_archive(&archive, &verify_dir)?;
    validate_extracted_database(&verify_dir)?;
    let media_root = if manifest.scope == BackupScope::Complete {
        verify_dir.join(MEDIA_DIR)
    } else {
        root.join(MEDIA_DIR)
    };
    verify_catalog_media_references(&verify_dir, &media_root, true)?;
    let (total_bytes, archive_hash) = file_digest(&archive)?;
    let summary = summary_from_manifest(&manifest, total_bytes, archive_hash);
    fs::remove_dir_all(verify_dir).map_err(|e| e.to_string())?;
    Ok(summary)
}

pub(crate) fn restore_backup_at(
    root: &Path,
    token: &str,
    confirmed: bool,
) -> Result<BackupSummary, String> {
    restore_with_options(root, token, confirmed, available_bytes, None)
}

fn restore_with_options(
    root: &Path,
    token: &str,
    confirmed: bool,
    space_probe: impl Fn(&Path) -> Result<u64, String>,
    fail_after_media_files: Option<usize>,
) -> Result<BackupSummary, String> {
    require_token(token)?;
    if !confirmed {
        return Err("Restoration requires explicit confirmation".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let staging = ensure_child_dir(&root, STAGING_DIR)?;
    let archive = staging.join(format!("backup-{token}.part"));
    reject_regular_file(&archive)?;
    let manifest = validate_archive(&archive)?;
    let stage = root.join(format!("Restore-{token}"));
    create_fresh_directory(&stage)?;
    extract_archive(&archive, &stage)?;
    validate_extracted_database(&stage)?;

    let stage_media = stage.join(MEDIA_DIR);
    let live_media = ensure_child_dir(&root, MEDIA_DIR)?;
    let reference_media = if manifest.scope == BackupScope::Complete {
        &stage_media
    } else {
        &live_media
    };
    verify_catalog_media_references(&stage, reference_media, true)?;
    validate_restore_collisions(&stage, &root, &manifest)?;

    let new_media_bytes = missing_restore_bytes(&stage, &root, &manifest)?;
    let restore_db_bytes = fs::metadata(stage.join(DB_FILE))
        .map_err(|e| e.to_string())?
        .len();
    let required = new_media_bytes
        .saturating_add(restore_db_bytes)
        .saturating_add(SPACE_RESERVE_BYTES);
    let available = space_probe(&root)?;
    if available < required {
        return Err(format!(
            "Insufficient storage to restore this backup: need approximately {required} bytes, {available} bytes available"
        ));
    }

    // Merge media without replacing or deleting any existing media. New files
    // are copied to same-directory temporary names and atomically renamed.
    let mut installed = 0_usize;
    for entry in &manifest.files {
        if let Some((folder, leaf)) = split_media_path(&entry.path) {
            let source = stage.join(folder).join(leaf);
            let destination_dir = ensure_child_dir(&root, folder)?;
            let destination = destination_dir.join(leaf);
            if destination.exists() {
                continue; // collision and checksum were validated above
            }
            install_verified_file(&source, &destination, token, entry)?;
            installed += 1;
            if fail_after_media_files == Some(installed) {
                return Err("Synthetic interrupted restore after media installation".into());
            }
        }
    }

    let staged_db = stage.join(DB_FILE);
    let live_db = root.join(DB_FILE);
    reject_destination_kind(&live_db)?;
    let replacement = root.join(format!(".{DB_FILE}.restore-{token}"));
    reject_absent_destination(&replacement)?;
    copy_new_file(&staged_db, &replacement)?;
    validate_db_integrity(&replacement)?;
    checkpoint_and_close_wal(&live_db)?;
    // Same-volume rename makes the database switch atomic. Existing media are
    // merged and never removed; before this point the old database stays live.
    fs::rename(&replacement, &live_db).map_err(|e| e.to_string())?;
    validate_db_integrity(&live_db)?;
    let (total_bytes, archive_hash) = file_digest(&archive)?;
    Ok(summary_from_manifest(&manifest, total_bytes, archive_hash))
}

fn validate_archive(path: &Path) -> Result<Manifest, String> {
    reject_regular_file(path)?;
    let mut input = File::open(path).map_err(|e| e.to_string())?;
    let manifest = read_manifest(&mut input)?;
    validate_manifest(&manifest)?;
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    for entry in &manifest.files {
        let mut remaining = entry.bytes;
        let mut hash = Sha256::new();
        while remaining > 0 {
            let wanted = usize::try_from(remaining.min(buffer.len() as u64)).unwrap();
            input
                .read_exact(&mut buffer[..wanted])
                .map_err(|_| "Backup is truncated".to_string())?;
            hash.update(&buffer[..wanted]);
            remaining -= wanted as u64;
        }
        if hex(&hash.finalize()) != entry.sha256 {
            return Err(format!("Backup checksum mismatch for {}", entry.path));
        }
    }
    let mut extra = [0_u8; 1];
    if input.read(&mut extra).map_err(|e| e.to_string())? != 0 {
        return Err("Backup has unexpected trailing data".into());
    }
    Ok(manifest)
}

fn read_manifest(reader: &mut impl Read) -> Result<Manifest, String> {
    let mut magic = [0_u8; 8];
    reader
        .read_exact(&mut magic)
        .map_err(|_| "Not a MyPod backup bundle".to_string())?;
    if &magic != MAGIC {
        return Err("Unsupported backup format".into());
    }
    let mut length_bytes = [0_u8; 8];
    reader
        .read_exact(&mut length_bytes)
        .map_err(|_| "Backup manifest header is truncated".to_string())?;
    let length = u64::from_le_bytes(length_bytes);
    if length == 0 || length > MAX_MANIFEST_BYTES {
        return Err("Backup manifest length is invalid".into());
    }
    let mut bytes = vec![0_u8; usize::try_from(length).map_err(|e| e.to_string())?];
    reader
        .read_exact(&mut bytes)
        .map_err(|_| "Backup manifest is truncated".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "Backup manifest is malformed".into())
}

fn validate_manifest(manifest: &Manifest) -> Result<(), String> {
    if manifest.format_version != FORMAT_VERSION {
        return Err("Unsupported backup format version".into());
    }
    if manifest.schema_version < 1 || manifest.schema_version > CURRENT_SCHEMA {
        return Err("Backup database schema version is unsupported by this app".into());
    }
    if manifest.file_count != manifest.files.len()
        || manifest.file_count == 0
        || manifest.file_count > MAX_BACKUP_FILES
    {
        return Err("Backup file count is invalid".into());
    }
    let mut paths = HashSet::new();
    let mut has_database = false;
    for entry in &manifest.files {
        validate_archive_path(&entry.path)?;
        if !paths.insert(entry.path.as_str()) {
            return Err("Backup contains duplicate file paths".into());
        }
        if entry.path == DB_FILE {
            has_database = true;
        }
        if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Backup contains an invalid checksum".into());
        }
    }
    if !has_database {
        return Err("Backup does not contain a database".into());
    }
    if manifest.scope == BackupScope::DatabaseOnly && manifest.files.len() != 1 {
        return Err("Database-only backup contains unexpected media".into());
    }
    Ok(())
}

fn validate_archive_path(path: &str) -> Result<(), String> {
    let path_obj = Path::new(path);
    if path_obj.is_absolute()
        || path.contains('\\')
        || path.contains('\0')
        || path_obj
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Backup contains a path that escapes its package".into());
    }
    if path == DB_FILE {
        return Ok(());
    }
    let (folder, leaf) = path
        .split_once('/')
        .ok_or_else(|| "Backup contains an unsupported path".to_string())?;
    if folder != MEDIA_DIR && folder != STAGING_DIR {
        return Err("Backup contains an unsupported directory".into());
    }
    validate_leaf(leaf)?;
    if folder == STAGING_DIR && !leaf.ends_with(".part") {
        return Err("Backup contains an unsupported staged-file name".into());
    }
    Ok(())
}

fn validate_leaf(leaf: &str) -> Result<(), String> {
    if leaf.is_empty()
        || leaf == "."
        || leaf == ".."
        || leaf.starts_with('.')
        || leaf.len() > 255
        || !leaf
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err("Backup contains an unsafe file name".into());
    }
    Ok(())
}

fn source_files(root: &Path, folder: &str) -> Result<Vec<(String, PathBuf)>, String> {
    let directory = root.join(folder);
    match fs::symlink_metadata(&directory) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err(format!("{folder} is not a safe app-managed directory"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    }
    let mut files = Vec::new();
    for item in fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let path = item.path();
        let file_type = item.file_type().map_err(|e| e.to_string())?;
        if file_type.is_symlink() {
            return Err(format!("Refusing to back up symlink in {folder}"));
        }
        if !file_type.is_file() {
            return Err(format!("Refusing unexpected directory entry in {folder}"));
        }
        let leaf = item.file_name().to_string_lossy().into_owned();
        // CSV preview files are ephemeral copies of a personal export. They
        // are intentionally excluded without opening or reading their data.
        if folder == STAGING_DIR && leaf.starts_with("spotify-") {
            continue;
        }
        validate_leaf(&leaf)?;
        if folder == STAGING_DIR && !leaf.ends_with(".part") {
            return Err("Unexpected file in the import staging directory".into());
        }
        files.push((format!("{folder}/{leaf}"), path));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

fn estimate_backup_bytes(root: &Path, scope: BackupScope) -> Result<u64, String> {
    let db = Connection::open(root.join(DB_FILE)).map_err(|e| e.to_string())?;
    let page_count: i64 = db
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let page_size: i64 = db
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    drop(db);
    let mut payload = (page_count.max(0) as u64)
        .saturating_mul(page_size.max(0) as u64)
        .saturating_mul(2);
    if scope == BackupScope::Complete {
        for folder in [MEDIA_DIR, STAGING_DIR] {
            for (_, path) in source_files(root, folder)? {
                payload =
                    payload.saturating_add(fs::metadata(path).map_err(|e| e.to_string())?.len());
            }
        }
    }
    Ok(payload
        .saturating_add(payload / 10)
        .saturating_add(SPACE_RESERVE_BYTES))
}

fn verify_catalog_media_references(
    database_root: &Path,
    media_root: &Path,
    require_audio_hash: bool,
) -> Result<(), String> {
    let db = Connection::open(database_root.join(DB_FILE)).map_err(|e| e.to_string())?;
    let mut statement = db
        .prepare("SELECT relative_path, content_sha256, file_size FROM imported_tracks")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?.max(0) as u64,
            ))
        })
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (relative, expected_hash, expected_bytes) = row.map_err(|e| e.to_string())?;
        validate_archive_path(&relative)?;
        if !relative.starts_with("Media/") {
            return Err("Catalog contains a media reference outside managed storage".into());
        }
        let file = media_root.join(&relative["Media/".len()..]);
        reject_regular_file(&file)?;
        if require_audio_hash {
            let (bytes, digest) = file_digest(&file)?;
            if bytes != expected_bytes || digest != expected_hash {
                return Err(format!(
                    "Managed media does not match catalog reference {relative}"
                ));
            }
        }
    }
    Ok(())
}

fn validate_extracted_database(root: &Path) -> Result<(), String> {
    let db_path = root.join(DB_FILE);
    reject_regular_file(&db_path)?;
    let db = open_catalog(root)?;
    validate_connection_integrity(&db)?;
    drop(db);
    Ok(())
}

fn validate_db_integrity(path: &Path) -> Result<(), String> {
    let db = Connection::open(path).map_err(|e| e.to_string())?;
    validate_connection_integrity(&db)
}

fn validate_connection_integrity(db: &Connection) -> Result<(), String> {
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if integrity != "ok" {
        return Err(format!(
            "Backup database failed integrity_check: {integrity}"
        ));
    }
    let foreign_key_failure: Option<String> = db
        .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    if foreign_key_failure.is_some() {
        return Err("Backup database contains foreign-key violations".into());
    }
    Ok(())
}

fn validate_restore_collisions(
    stage: &Path,
    root: &Path,
    manifest: &Manifest,
) -> Result<(), String> {
    for entry in &manifest.files {
        if let Some((folder, leaf)) = split_media_path(&entry.path) {
            let target = root.join(folder).join(leaf);
            match fs::symlink_metadata(&target) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_file() {
                        return Err(format!(
                            "Restore destination is not a regular managed file: {}",
                            entry.path
                        ));
                    }
                    let (bytes, digest) = file_digest(&target)?;
                    if bytes != entry.bytes || digest != entry.sha256 {
                        return Err(format!(
                            "Restore would collide with different existing media: {}",
                            entry.path
                        ));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
    }
    // Reject any archive members that were not extracted into the expected
    // app-private paths, including paths introduced by a malformed manifest.
    for entry in &manifest.files {
        let extracted = stage.join(&entry.path);
        reject_regular_file(&extracted)?;
    }
    Ok(())
}

fn missing_restore_bytes(stage: &Path, root: &Path, manifest: &Manifest) -> Result<u64, String> {
    let mut bytes = 0_u64;
    for entry in &manifest.files {
        if let Some((folder, leaf)) = split_media_path(&entry.path) {
            if !root.join(folder).join(leaf).exists() {
                bytes = bytes.saturating_add(entry.bytes);
            }
        }
    }
    let _ = stage;
    Ok(bytes)
}

fn install_verified_file(
    source: &Path,
    destination: &Path,
    token: &str,
    entry: &ManifestFile,
) -> Result<(), String> {
    let temp = destination.with_file_name(format!(
        ".restore-{token}-{}",
        destination.file_name().unwrap().to_string_lossy()
    ));
    reject_absent_destination(&temp)?;
    copy_new_file(source, &temp)?;
    let (bytes, digest) = file_digest(&temp)?;
    if bytes != entry.bytes || digest != entry.sha256 {
        return Err("Restored media checksum changed while copying".into());
    }
    fs::rename(temp, destination).map_err(|e| e.to_string())
}

fn extract_archive(archive: &Path, destination_root: &Path) -> Result<(), String> {
    let mut input = File::open(archive).map_err(|e| e.to_string())?;
    let manifest = read_manifest(&mut input)?;
    validate_manifest(&manifest)?;
    for entry in &manifest.files {
        validate_archive_path(&entry.path)?;
        let output_path = destination_root.join(&entry.path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        reject_absent_destination(&output_path)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .map_err(|e| e.to_string())?;
        let mut remaining = entry.bytes;
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; COPY_BUFFER_BYTES];
        while remaining > 0 {
            let wanted = usize::try_from(remaining.min(buffer.len() as u64)).unwrap();
            input
                .read_exact(&mut buffer[..wanted])
                .map_err(|_| "Backup is truncated".to_string())?;
            output
                .write_all(&buffer[..wanted])
                .map_err(|e| e.to_string())?;
            hash.update(&buffer[..wanted]);
            remaining -= wanted as u64;
        }
        output.sync_all().map_err(|e| e.to_string())?;
        if hex(&hash.finalize()) != entry.sha256 {
            return Err(format!("Backup checksum mismatch for {}", entry.path));
        }
    }
    let mut extra = [0_u8; 1];
    if input.read(&mut extra).map_err(|e| e.to_string())? != 0 {
        return Err("Backup has unexpected trailing data".into());
    }
    Ok(())
}

fn append_verified_file(
    source: &Path,
    output: &mut impl Write,
    entry: &ManifestFile,
    written: &mut u64,
    fail_after: Option<u64>,
) -> Result<(), String> {
    let mut input = File::open(source).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    let mut bytes = 0_u64;
    loop {
        let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        if fail_after.is_some_and(|limit| *written >= limit) {
            return Err("Synthetic interrupted backup write".into());
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        hash.update(&buffer[..count]);
        bytes = bytes.saturating_add(count as u64);
        *written = written.saturating_add(count as u64);
    }
    if bytes != entry.bytes || hex(&hash.finalize()) != entry.sha256 {
        return Err(format!("Source changed while backing up {}", entry.path));
    }
    Ok(())
}

fn file_digest(path: &Path) -> Result<(u64, String), String> {
    reject_regular_file(path)?;
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        bytes = bytes.saturating_add(count as u64);
    }
    Ok((bytes, hex(&hash.finalize())))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn summary_from_manifest(manifest: &Manifest, total_bytes: u64, sha256: String) -> BackupSummary {
    BackupSummary {
        format_version: manifest.format_version,
        schema_version: manifest.schema_version,
        journal_mode: manifest.journal_mode.clone(),
        scope: manifest.scope,
        file_count: manifest.file_count,
        total_bytes,
        sha256,
    }
}

fn split_media_path(path: &str) -> Option<(&str, &str)> {
    let (folder, leaf) = path.split_once('/')?;
    (folder == MEDIA_DIR || folder == STAGING_DIR).then_some((folder, leaf))
}

fn require_token(token: &str) -> Result<(), String> {
    if token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("Invalid backup token".into())
    }
}

fn ensure_child_dir(root: &Path, name: &str) -> Result<PathBuf, String> {
    let path = root.join(name);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("{name} is not a safe app-managed directory"));
    }
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    if !canonical.starts_with(root) {
        return Err("Backup directory escaped app-private storage".into());
    }
    Ok(canonical)
}

fn create_fresh_directory(path: &Path) -> Result<(), String> {
    fs::create_dir(path)
        .map_err(|e| format!("Could not prepare an isolated restore workspace: {e}"))?;
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Restore workspace is not a regular directory".into());
    }
    Ok(())
}

fn reject_absent_destination(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err("Backup or restore destination already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn reject_destination_kind(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err("Database destination is not a regular file".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn reject_regular_file(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Expected a regular, non-symlink file".into());
    }
    Ok(())
}

fn copy_new_file(source: &Path, destination: &Path) -> Result<(), String> {
    reject_regular_file(source)?;
    let mut input = File::open(source).map_err(|e| e.to_string())?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| e.to_string())?;
    std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
    output.sync_all().map_err(|e| e.to_string())
}

fn checkpoint_and_close_wal(db_path: &Path) -> Result<(), String> {
    if !db_path.exists() {
        return Ok(());
    }
    let connection = Connection::open(db_path).map_err(|e| e.to_string())?;
    let mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if mode.eq_ignore_ascii_case("wal") {
        let (busy, _, _): (i64, i64, i64) = connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|e| e.to_string())?;
        if busy != 0 {
            return Err("Could not safely checkpoint the existing SQLite WAL".into());
        }
        let result: String = connection
            .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if !result.eq_ignore_ascii_case("delete") {
            return Err("Could not close the existing SQLite WAL before restore".into());
        }
    }
    drop(connection);
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", db_path.to_string_lossy()));
        if sidecar.exists() {
            return Err("SQLite sidecar remains; database replacement was stopped safely".into());
        }
    }
    Ok(())
}

fn available_bytes(path: &Path) -> Result<u64, String> {
    let text = path
        .to_str()
        .ok_or_else(|| "Storage path is not valid UTF-8".to_string())?;
    let c_path = CString::new(text).map_err(|e| e.to_string())?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::statvfs(c_path.as_ptr(), &mut stats) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok((stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ios_catalog::{register_import_at, resolve_track_audio_path_at};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "mypod-backup-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = base.join("MyPod");
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn remove_root(root: &Path) {
        if let Some(base) = root.parent() {
            let _ = fs::remove_dir_all(base);
        }
    }

    fn synthetic_wav(path: &Path, sample: u8) {
        let mut bytes = Vec::new();
        let samples = vec![sample; 96];
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

    fn token(n: u8) -> String {
        format!("{n:02x}").repeat(16)
    }

    fn add_track(root: &Path, token: &str, title: &str, sample: u8) -> i64 {
        let importing = root.join(STAGING_DIR);
        fs::create_dir_all(&importing).unwrap();
        synthetic_wav(&importing.join(format!("{token}.part")), sample);
        register_import_at(
            root.parent().unwrap(),
            token,
            &format!("{title}.wav"),
            "wav",
        )
        .unwrap()
        .track
        .id
    }

    fn stage_restore_archive(root: &Path, archive: &Path, token: &str) {
        let importing = root.join(STAGING_DIR);
        fs::create_dir_all(&importing).unwrap();
        fs::copy(archive, importing.join(format!("backup-{token}.part"))).unwrap();
    }

    fn db_counts(root: &Path) -> (i64, i64, i64) {
        let db = open_catalog(root).unwrap();
        (
            db.query_row("SELECT count(*) FROM imported_tracks", [], |r| r.get(0))
                .unwrap(),
            db.query_row("SELECT count(*) FROM playlists", [], |r| r.get(0))
                .unwrap(),
            db.query_row("SELECT count(*) FROM catalog_tracks", [], |r| r.get(0))
                .unwrap(),
        )
    }

    fn make_backup(root: &Path, backup_token: &str, scope: BackupScope) -> PathBuf {
        create_backup_at(root, backup_token, scope).unwrap();
        root.join(BACKUP_DIR)
            .join(format!("{backup_token}.mypodbackup"))
    }

    #[test]
    fn backs_up_and_restores_empty_and_populated_libraries() {
        let empty = temp_root("empty");
        open_catalog(&empty).unwrap();
        let archive = make_backup(&empty, &token(1), BackupScope::Complete);
        let target = temp_root("empty-restore");
        fs::create_dir_all(target.join(STAGING_DIR)).unwrap();
        stage_restore_archive(&target, &archive, &token(29));
        let inspected = inspect_backup_at(&target, &token(29)).unwrap();
        assert_eq!(inspected.scope, BackupScope::Complete);
        assert_eq!(inspected.schema_version, CURRENT_SCHEMA);
        stage_restore_archive(&target, &archive, &token(2));
        let summary = restore_backup_at(&target, &token(2), true).unwrap();
        assert_eq!(summary.scope, BackupScope::Complete);
        assert_eq!(db_counts(&target), (0, 0, 0));

        let source = temp_root("populated");
        let track_id = add_track(&source, &token(3), "Synthetic", 1);
        let playlist = crate::ios_playlists::create_playlist_at(
            source.parent().unwrap(),
            "Synthetic playlist",
        )
        .unwrap();
        crate::ios_playlists::add_track_at(source.parent().unwrap(), playlist.id, track_id)
            .unwrap();
        crate::ios_playlists::add_track_at(source.parent().unwrap(), playlist.id, track_id)
            .unwrap();
        crate::ios_playlists::create_playlist_at(source.parent().unwrap(), "Synthetic playlist")
            .unwrap();
        let db = open_catalog(&source).unwrap();
        db.execute(
            "INSERT INTO catalog_tracks(title,artist,album,spotify_id,isrc,local_identifier) VALUES('Metadata only','Artist','Album','opaque','ISRC','local-1')",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO spotify_imports(source_digest,row_count) VALUES('digest',1)",
            [],
        )
        .unwrap();
        drop(db);
        let source_db_hash_before = file_digest(&source.join(DB_FILE)).unwrap();
        let archive = make_backup(&source, &token(4), BackupScope::Complete);
        assert_eq!(
            file_digest(&source.join(DB_FILE)).unwrap(),
            source_db_hash_before,
            "backup must not modify the source database"
        );
        let target = temp_root("populated-restore");
        fs::create_dir_all(target.join(STAGING_DIR)).unwrap();
        stage_restore_archive(&target, &archive, &token(5));
        restore_backup_at(&target, &token(5), true).unwrap();
        assert_eq!(db_counts(&target), (1, 2, 1));
        let source_db = open_catalog(&source).unwrap();
        let target_db = open_catalog(&target).unwrap();
        let source_playlists: Vec<String> = source_db
            .prepare("SELECT name FROM playlists ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let target_playlists: Vec<String> = target_db
            .prepare("SELECT name FROM playlists ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            target_playlists, source_playlists,
            "same-named playlists remain separate"
        );
        let source_entries: Vec<(i64, i64)> = source_db
            .prepare(
                "SELECT position, track_id FROM playlist_entries ORDER BY playlist_id, position",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let target_entries: Vec<(i64, i64)> = target_db
            .prepare(
                "SELECT position, track_id FROM playlist_entries ORDER BY playlist_id, position",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            target_entries, source_entries,
            "duplicate ordered entries survive restore"
        );
        let catalog_metadata: (String, String, String) = target_db
            .query_row(
                "SELECT title, spotify_id, local_identifier FROM catalog_tracks",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            catalog_metadata,
            ("Metadata only".into(), "opaque".into(), "local-1".into())
        );
        let restored_track =
            resolve_track_audio_path_at(target.parent().unwrap(), track_id).unwrap();
        assert_eq!(
            file_digest(&restored_track).unwrap(),
            file_digest(&source.join(MEDIA_DIR).join(format!("{}.wav", token(3)))).unwrap()
        );
        assert!(target
            .join(MEDIA_DIR)
            .join(format!("{}.wav", token(3)))
            .is_file());

        remove_root(&empty);
        remove_root(&target);
        remove_root(&source);
    }

    #[test]
    fn database_only_backup_is_distinct_and_restore_requires_media_to_exist() {
        let source = temp_root("db-only-source");
        let track_id = add_track(&source, &token(6), "Audio", 2);
        let archive = make_backup(&source, &token(7), BackupScope::DatabaseOnly);
        let manifest = validate_archive(&archive).unwrap();
        assert_eq!(manifest.scope, BackupScope::DatabaseOnly);
        assert_eq!(manifest.file_count, 1);
        let target = temp_root("db-only-target");
        fs::create_dir_all(target.join(STAGING_DIR)).unwrap();
        stage_restore_archive(&target, &archive, &token(8));
        assert!(restore_backup_at(&target, &token(8), true).is_err());
        fs::create_dir_all(target.join(MEDIA_DIR)).unwrap();
        fs::copy(
            source.join(MEDIA_DIR).join(format!("{}.wav", token(6))),
            target.join(MEDIA_DIR).join(format!("{}.wav", token(6))),
        )
        .unwrap();
        stage_restore_archive(&target, &archive, &token(28));
        restore_backup_at(&target, &token(28), true).unwrap();
        assert!(
            resolve_track_audio_path_at(target.parent().unwrap(), track_id)
                .unwrap()
                .is_file()
        );
        remove_root(&source);
        remove_root(&target);
    }

    #[test]
    fn wal_committed_rows_are_included_in_a_consistent_snapshot() {
        let source = temp_root("wal-source");
        let db = open_catalog(&source).unwrap();
        let mode: String = db
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "wal");
        db.execute_batch("PRAGMA wal_autocheckpoint=0; INSERT INTO playlists(name,created_at,updated_at) VALUES('WAL playlist',1,1);").unwrap();
        let archive = make_backup(&source, &token(9), BackupScope::DatabaseOnly);
        assert_eq!(
            validate_archive(&archive).unwrap().scope,
            BackupScope::DatabaseOnly
        );
        let target = temp_root("wal-target");
        fs::create_dir_all(target.join(STAGING_DIR)).unwrap();
        stage_restore_archive(&target, &archive, &token(10));
        restore_backup_at(&target, &token(10), true).unwrap();
        assert_eq!(db_counts(&target).1, 1);
        drop(db);
        remove_root(&source);
        remove_root(&target);
    }

    #[test]
    fn interrupted_backup_is_not_published_and_source_is_unchanged() {
        let root = temp_root("interrupted-backup");
        add_track(&root, &token(11), "Keep", 3);
        let media = root.join(MEDIA_DIR).join(format!("{}.wav", token(11)));
        let before = file_digest(&media).unwrap();
        let result = create_backup_with_options(
            &root,
            &token(12),
            BackupScope::Complete,
            |_| Ok(u64::MAX),
            Some(1),
        );
        assert!(result.unwrap_err().contains("interrupted"));
        assert!(!root
            .join(BACKUP_DIR)
            .join(format!("{}.mypodbackup", token(12)))
            .exists());
        assert!(root
            .join(BACKUP_DIR)
            .join(format!("{}.partial", token(12)))
            .is_file());
        assert_eq!(file_digest(&media).unwrap(), before);
        remove_root(&root);
    }

    #[test]
    fn rejects_corruption_unsupported_versions_and_escaping_paths() {
        let root = temp_root("corrupt-backup");
        open_catalog(&root).unwrap();
        let archive = make_backup(&root, &token(13), BackupScope::Complete);
        let mut bytes = fs::read(&archive).unwrap();
        *bytes.last_mut().unwrap() ^= 0xff;
        let corrupt = root.join("corrupt.mypodbackup");
        fs::write(&corrupt, bytes).unwrap();
        assert!(validate_archive(&corrupt).unwrap_err().contains("checksum"));
        let manifest = Manifest {
            format_version: FORMAT_VERSION + 1,
            schema_version: CURRENT_SCHEMA,
            journal_mode: "delete".into(),
            scope: BackupScope::DatabaseOnly,
            file_count: 1,
            files: vec![ManifestFile {
                path: DB_FILE.into(),
                bytes: 0,
                sha256: "0".repeat(64),
            }],
        };
        assert!(validate_manifest(&manifest).unwrap_err().contains("format"));
        let mut traversal = manifest;
        traversal.format_version = FORMAT_VERSION;
        traversal.files[0].path = "../../outside".into();
        assert!(validate_manifest(&traversal).unwrap_err().contains("path"));
        remove_root(&root);
    }

    #[test]
    fn interrupted_restore_keeps_live_database_and_all_media() {
        let source = temp_root("restore-source");
        add_track(&source, &token(14), "Backup", 4);
        let archive = make_backup(&source, &token(15), BackupScope::Complete);
        let target = temp_root("restore-target");
        let keep_id = add_track(&target, &token(16), "Existing", 5);
        let before_counts = db_counts(&target);
        stage_restore_archive(&target, &archive, &token(17));
        let result = restore_with_options(&target, &token(17), true, |_| Ok(u64::MAX), Some(1));
        assert!(result.unwrap_err().contains("interrupted restore"));
        assert_eq!(db_counts(&target), before_counts);
        assert!(
            resolve_track_audio_path_at(target.parent().unwrap(), keep_id)
                .unwrap()
                .is_file()
        );
        assert!(target
            .join(MEDIA_DIR)
            .join(format!("{}.wav", token(14)))
            .is_file());
        remove_root(&source);
        remove_root(&target);
    }

    #[test]
    fn insufficient_storage_and_unconfirmed_restore_leave_destination_unchanged() {
        let root = temp_root("space");
        add_track(&root, &token(18), "Keep", 6);
        let before = db_counts(&root);
        assert!(create_backup_with_options(
            &root,
            &token(19),
            BackupScope::Complete,
            |_| Ok(0),
            None
        )
        .unwrap_err()
        .contains("Insufficient storage"));
        let archive = make_backup(&root, &token(20), BackupScope::Complete);
        let target = temp_root("space-target");
        add_track(&target, &token(21), "Destination", 7);
        let before_target = db_counts(&target);
        stage_restore_archive(&target, &archive, &token(22));
        assert!(restore_with_options(&target, &token(22), false, |_| Ok(u64::MAX), None).is_err());
        assert!(
            restore_with_options(&target, &token(22), true, |_| Ok(0), None)
                .unwrap_err()
                .contains("Insufficient storage")
        );
        assert_eq!(db_counts(&target), before_target);
        assert_eq!(db_counts(&root), before);
        remove_root(&root);
        remove_root(&target);
    }

    #[test]
    fn identical_media_is_reused_and_different_collision_is_rejected() {
        let source = temp_root("collision-source");
        add_track(&source, &token(23), "Same", 8);
        let archive = make_backup(&source, &token(24), BackupScope::Complete);
        let target = temp_root("collision-target");
        fs::create_dir_all(target.join(STAGING_DIR)).unwrap();
        fs::create_dir_all(target.join(MEDIA_DIR)).unwrap();
        fs::copy(
            source.join(MEDIA_DIR).join(format!("{}.wav", token(23))),
            target.join(MEDIA_DIR).join(format!("{}.wav", token(23))),
        )
        .unwrap();
        stage_restore_archive(&target, &archive, &token(25));
        restore_backup_at(&target, &token(25), true).unwrap();
        assert_eq!(db_counts(&target).0, 1);

        let target2 = temp_root("collision-target-different");
        fs::create_dir_all(target2.join(STAGING_DIR)).unwrap();
        fs::create_dir_all(target2.join(MEDIA_DIR)).unwrap();
        synthetic_wav(
            &target2.join(MEDIA_DIR).join(format!("{}.wav", token(23))),
            9,
        );
        stage_restore_archive(&target2, &archive, &token(26));
        assert!(restore_backup_at(&target2, &token(26), true)
            .unwrap_err()
            .contains("collide"));
        assert_eq!(db_counts(&target2).0, 0);
        let existing = temp_root("restore-retains-existing-media");
        add_track(&existing, &token(30), "Keep file", 10);
        let existing_media = existing.join(MEDIA_DIR).join(format!("{}.wav", token(30)));
        let existing_hash = file_digest(&existing_media).unwrap();
        stage_restore_archive(&existing, &archive, &token(31));
        restore_backup_at(&existing, &token(31), true).unwrap();
        assert!(
            existing_media.is_file(),
            "successful restore must retain unrelated managed audio"
        );
        assert_eq!(file_digest(&existing_media).unwrap(), existing_hash);
        remove_root(&source);
        remove_root(&target);
        remove_root(&target2);
        remove_root(&existing);
    }

    #[test]
    fn backup_schema_compatibility_accepts_current_and_rejects_newer_schema() {
        let root = temp_root("schema-version");
        open_catalog(&root).unwrap();
        let archive = make_backup(&root, &token(27), BackupScope::DatabaseOnly);
        let mut manifest = validate_archive(&archive).unwrap();
        manifest.schema_version = CURRENT_SCHEMA + 1;
        assert!(validate_manifest(&manifest)
            .unwrap_err()
            .contains("schema version"));
        remove_root(&root);
    }
}
