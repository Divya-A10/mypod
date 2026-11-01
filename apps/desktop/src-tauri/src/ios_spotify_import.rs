//! Transactional, explicitly confirmed metadata import. No original files are opened here.
use crate::ios_catalog::{app_root, open_catalog};
use crate::ios_spotify_preview::SpotifyCsvPreview;
use rusqlite::{params, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::Path};

#[cfg(target_os = "ios")]
#[derive(Default)]
pub(crate) struct PendingImport(pub std::sync::Mutex<Option<(String, SpotifyCsvPreview)>>);

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct ImportResult {
    pub already_imported: bool,
    pub playlists: usize,
    pub entries: usize,
    pub preserved_other_rows: usize,
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn confirm_spotify_import(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    pending: tauri::State<'_, PendingImport>,
    import_id: String,
) -> Result<ImportResult, String> {
    use tauri::Manager;
    let preview = pending
        .0
        .lock()
        .map_err(|_| "Import preview is busy")?
        .as_ref()
        .filter(|(token, _)| token == &import_id)
        .map(|(_, p)| p.clone())
        .ok_or("Preview expired. Select the CSV again.")?;
    let root = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let lock = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().map_err(|_| "Catalog is busy")?;
        import_at(&root, &preview)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) fn import_at(root: &Path, preview: &SpotifyCsvPreview) -> Result<ImportResult, String> {
    if !preview.complete
        || !preview.file_errors.is_empty()
        || preview.rows.iter().any(|r| !r.errors.is_empty())
    {
        return Err("Fix all CSV errors before importing. No rows were imported.".into());
    }
    // Length framing makes this parsed-source fingerprint unambiguous, including source order.
    // It deliberately ignores CSV quoting/newline style, making equivalent exports idempotent.
    let mut hash = Sha256::new();
    for cell in preview
        .columns
        .iter()
        .chain(preview.rows.iter().flat_map(|r| r.source_values.iter()))
    {
        hash.update((cell.len() as u64).to_le_bytes());
        hash.update(cell.as_bytes());
    }
    let digest = hash
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let root = app_root(root)?;
    let mut db = open_catalog(&root)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM spotify_imports WHERE source_digest=?1)",
            [&digest],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists {
        return Ok(ImportResult {
            already_imported: true,
            playlists: 0,
            entries: 0,
            preserved_other_rows: 0,
        });
    }
    tx.execute(
        "INSERT INTO spotify_imports(source_digest,row_count) VALUES(?1,?2)",
        params![digest, preview.rows.len() as i64],
    )
    .map_err(|e| e.to_string())?;
    let batch = tx.last_insert_rowid();
    let mut catalog = HashMap::new();
    let mut group: Option<(String, i64, i64)> = None;
    let mut result = ImportResult {
        already_imported: false,
        playlists: 0,
        entries: 0,
        preserved_other_rows: 0,
    };
    for row in &preview.rows {
        let m = &row.mapped;
        let kind = m.source_type.as_deref().unwrap_or("");
        let label = m.playlist_name.as_deref().unwrap_or("");
        let title = m.track_name.as_deref().unwrap_or("");
        let mut catalog_id = None;
        // Artist and Album rows are source entities, not necessarily recordings.
        if matches!(kind, "Playlist" | "Favorite") && !title.trim().is_empty() {
            // Source identity alone is insufficient when metadata conflicts. Keep versions separate.
            let key = (
                m.spotify_id.clone(),
                m.track_name.clone(),
                m.artist_name.clone(),
                m.album.clone(),
                m.isrc.clone(),
            );
            let id = if let Some(id) = catalog.get(&key) {
                *id
            } else {
                tx.execute("INSERT INTO catalog_tracks(title,artist,album,spotify_id,isrc) VALUES(?1,?2,?3,?4,?5)", params![title,m.artist_name,m.album,m.spotify_id,m.isrc]).map_err(|e| e.to_string())?;
                let id = tx.last_insert_rowid();
                tx.execute(
                    "UPDATE catalog_tracks SET local_identifier=?1 WHERE id=?2",
                    params![format!("spotify-{id}"), id],
                )
                .map_err(|e| e.to_string())?;
                catalog.insert(key, id);
                id
            };
            catalog_id = Some(id);
        }
        let mut entry_id = None;
        if kind == "Playlist" && !label.trim().is_empty() {
            if group.as_ref().map(|g| g.0.as_str()) != Some(label) {
                let name: String = label.trim().chars().take(120).collect();
                tx.execute("INSERT INTO playlists(name,created_at,updated_at,source_playlist_label,source_playlist_identifier) VALUES(?1,strftime('%s','now'),strftime('%s','now'),?2,?3)", params![name,label,format!("spotify-import:{batch}:block:{}",result.playlists+1)]).map_err(|e| e.to_string())?;
                group = Some((label.to_string(), tx.last_insert_rowid(), 0));
                result.playlists += 1;
            }
            let (_, playlist, position) = group.as_mut().unwrap();
            tx.execute("INSERT INTO playlist_entries(playlist_id,position,title_snapshot,artist_snapshot,album_snapshot,source_type_label,source_identifier,catalog_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![*playlist,*position,title,m.artist_name,m.album,kind,m.spotify_id,catalog_id]).map_err(|e| e.to_string())?;
            entry_id = Some(tx.last_insert_rowid());
            *position += 1;
            result.entries += 1;
        } else {
            group = None;
            result.preserved_other_rows += 1;
        }
        // Store cells with length framing; lossless even with tabs, quotes or embedded newlines.
        let cells = row
            .source_values
            .iter()
            .map(|v| format!("{}:{v}", v.len()))
            .collect::<String>();
        tx.execute("INSERT INTO spotify_rows(batch_id,source_record,line_start,line_end,source_type,playlist_label,catalog_id,entry_id,source_cells) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![batch,row.source_record as i64,row.physical_line_start as i64,row.physical_line_end as i64,kind,label,catalog_id,entry_id,cells]).map_err(|e| e.to_string())?;
    }
    crate::ios_audio_matching::auto_match(&tx, &root)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ios_spotify_preview::parse_spotify_csv;
    const H: &str = "Track name,Artist name,Album,Playlist name,Type,ISRC,Spotify - id\n";
    fn root() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "mypod-csv-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn grouping_repeats_nonplaylist_rows_and_idempotency() {
        let root = root();
        let existing = crate::ios_playlists::create_playlist_at(&root, "Mix").unwrap();
        let p = parse_spotify_csv(format!("{H}Song,A,B,Mix,Playlist,,id\nSong,A,B,Mix,Playlist,,id\nSong,A,B,Other,Playlist,,id\nSong,A,B,Mix,Playlist,,id\nSaved,A,B,,Favorite,,id\nAlbum,A,B,,Album,,a\nArtist,A,,,Artist,,b\nLost,A,B,,Playlist,,c\n").as_bytes());
        let r = import_at(&root, &p).unwrap();
        assert_eq!((r.playlists, r.entries, r.preserved_other_rows), (3, 4, 4));
        let lists = crate::ios_playlists::list_playlists_at(&root).unwrap();
        assert_eq!(lists.len(), 4);
        assert_eq!(
            crate::ios_playlists::list_entries_at(&root, existing.id)
                .unwrap()
                .len(),
            0
        );
        assert!(import_at(&root, &p).unwrap().already_imported);
        let db = open_catalog(&app_root(&root).unwrap()).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM catalog_tracks", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM playlist_entries WHERE track_id IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "Opt-in read-only validation; set MYPOD_VALIDATION_CSV to a local export"]
    fn local_export_reconstruction() {
        let path = std::env::var_os("MYPOD_VALIDATION_CSV").expect("Set validation path");
        let before = std::fs::read(&path).unwrap();
        let p = parse_spotify_csv(&before);
        assert!(p.complete && p.file_errors.is_empty());
        assert_eq!(p.summary.source_records, 6596);
        let root = root();
        let r = import_at(&root, &p).unwrap();
        assert_eq!(
            (r.playlists, r.entries, r.preserved_other_rows),
            (61, 5880, 716)
        );
        let db = open_catalog(&app_root(&root).unwrap()).unwrap();
        let mut statement = db.prepare("SELECT s.source_record, e.position, e.title_snapshot, e.source_identifier FROM spotify_rows s JOIN playlist_entries e ON e.id=s.entry_id ORDER BY s.source_record").unwrap();
        let actual = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let expected = p
            .rows
            .iter()
            .filter(|r| r.mapped.source_type.as_deref() == Some("Playlist"))
            .collect::<Vec<_>>();
        let mut previous = None;
        let mut position = 0;
        for (a, e) in actual.iter().zip(expected) {
            if previous != e.mapped.playlist_name.as_deref() {
                position = 0;
            }
            assert!(
                a.0 == e.source_record as i64
                    && a.1 == position
                    && Some(a.2.as_str()) == e.mapped.track_name.as_deref()
                    && a.3 == e.mapped.spotify_id,
                "Source membership or order mismatch"
            );
            previous = e.mapped.playlist_name.as_deref();
            position += 1;
        }
        assert!(import_at(&root, &p).unwrap().already_imported);
        assert!(
            std::fs::read(&path).unwrap() == before,
            "Source file changed"
        );
        drop(statement);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_input_and_mid_import_failure_roll_back() {
        let root = root();
        assert!(import_at(&root, &parse_spotify_csv(b"bad")).is_err());
        let db = open_catalog(&app_root(&root).unwrap()).unwrap();
        db.execute_batch("CREATE TRIGGER fail_row BEFORE INSERT ON spotify_rows BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
        let p = parse_spotify_csv(format!("{H}Song,A,B,Mix,Playlist,,id\n").as_bytes());
        assert!(import_at(&root, &p).is_err());
        for table in [
            "spotify_imports",
            "catalog_tracks",
            "playlists",
            "playlist_entries",
        ] {
            assert_eq!(
                db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
