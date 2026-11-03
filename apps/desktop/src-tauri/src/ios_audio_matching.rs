//! Conservative embedded-tag matching. Filename guesses never qualify for automatic links.
use crate::ios_catalog::{app_root, open_catalog, resolve_track_audio_path_at};
use lofty::{
    config::ParseOptions,
    file::TaggedFileExt,
    probe::Probe,
    tag::{Accessor, ItemKey},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct Metadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub isrc: Option<String>,
    pub error: Option<String>,
}
pub(crate) fn read_metadata(path: &Path) -> Metadata {
    lofty::config::apply_global_options(
        lofty::config::GlobalOptions::new()
            .allocation_limit(2 * 1024 * 1024)
            .use_custom_resolvers(false),
    );
    let result = (|| {
        let tagged = Probe::open(path)?
            .guess_file_type()?
            .options(
                ParseOptions::new()
                    .read_properties(false)
                    .read_cover_art(false),
            )
            .read()?;
        let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
            return Ok(Metadata::default());
        };
        Ok::<_, lofty::error::LoftyError>(Metadata {
            title: tag.title().map(|v| v.into_owned()),
            artist: tag.artist().map(|v| v.into_owned()),
            album: tag.album().map(|v| v.into_owned()),
            isrc: tag.get_string(&ItemKey::Isrc).map(str::to_string),
            error: None,
        })
    })();
    result.unwrap_or_else(|_| Metadata {
        error: Some("Embedded tags could not be read; review the file manually.".into()),
        ..Metadata::default()
    })
}

/// Read only the first supported, bounded embedded image. Callers provide an
/// already validated app-managed audio path and never expose that path to JS.
pub(crate) fn read_cover_art(path: &Path) -> Option<(Vec<u8>, String)> {
    const MAX_ARTWORK_BYTES: usize = 8 * 1024 * 1024;
    lofty::config::apply_global_options(
        lofty::config::GlobalOptions::new()
            .allocation_limit(MAX_ARTWORK_BYTES)
            .use_custom_resolvers(false),
    );
    let tagged = Probe::open(path)
        .ok()?
        .guess_file_type()
        .ok()?
        .options(
            ParseOptions::new()
                .read_properties(false)
                .read_cover_art(true),
        )
        .read()
        .ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    tag.pictures().iter().find_map(|picture| {
        let data = picture.data();
        if data.is_empty() || data.len() > MAX_ARTWORK_BYTES {
            return None;
        }
        let mime = if data.starts_with(b"\xff\xd8\xff") {
            "image/jpeg"
        } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
            "image/gif"
        } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
            "image/webp"
        } else {
            return None;
        };
        Some((data.to_vec(), mime.to_string()))
    })
}
fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn same(a: &Option<String>, b: &Option<String>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => !a.trim().is_empty() && norm(a) == norm(b),
        _ => false,
    }
}
fn isrc(s: &Option<String>) -> Option<String> {
    let v = s.as_ref()?.replace('-', "").to_ascii_uppercase();
    let b = v.as_bytes();
    (b.len() == 12
        && b[..2].iter().all(u8::is_ascii_alphabetic)
        && b[2..5].iter().all(u8::is_ascii_alphanumeric)
        && b[5..].iter().all(u8::is_ascii_digit))
    .then_some(v)
}
fn strong(a: &Metadata, b: &Metadata) -> bool {
    isrc(&a.isrc).is_some()
        && isrc(&a.isrc) == isrc(&b.isrc)
        && same(&a.title, &b.title)
        && same(&a.artist, &b.artist)
        && same(&a.album, &b.album)
}
pub(crate) fn save_metadata(db: &Connection, track: i64, m: &Metadata) -> Result<(), String> {
    db.execute("INSERT OR REPLACE INTO audio_metadata(track_id,title,artist,album,isrc,read_error) VALUES(?1,?2,?3,?4,?5,?6)",params![track,m.title,m.artist,m.album,m.isrc,m.error]).map_err(|e|e.to_string())?;
    Ok(())
}
fn local_metadata(db: &Connection) -> Result<Vec<(i64, Metadata)>, String> {
    let mut q=db.prepare("SELECT track_id,title,artist,album,isrc,read_error FROM audio_metadata ORDER BY track_id").map_err(|e|e.to_string())?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                Metadata {
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    album: r.get(3)?,
                    isrc: r.get(4)?,
                    error: r.get(5)?,
                },
            ))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
fn catalog_metadata(db: &Connection) -> Result<Vec<(i64, Metadata)>, String> {
    let mut q = db
        .prepare("SELECT id,title,artist,album,isrc FROM catalog_tracks ORDER BY id")
        .map_err(|e| e.to_string())?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                Metadata {
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    album: r.get(3)?,
                    isrc: r.get(4)?,
                    error: None,
                },
            ))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
/// Called in the surrounding import transaction. Manual unlink suppression is permanent until confirmation.
pub(crate) fn auto_match(db: &Connection, media_root: &Path) -> Result<(), String> {
    db.execute("DELETE FROM audio_matches WHERE method='automatic'", [])
        .map_err(|e| e.to_string())?;
    let locals = local_metadata(db)?
        .into_iter()
        .filter(|(id, _)| {
            db.query_row(
                "SELECT relative_path FROM imported_tracks WHERE id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .is_some_and(|p| crate::ios_catalog::managed_file_exists(media_root, &p))
        })
        .collect::<Vec<_>>();
    let catalog = catalog_metadata(db)?;
    for (cid, cm) in &catalog {
        let candidates = locals
            .iter()
            .filter(|(_, m)| strong(cm, m))
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            continue;
        }
        let (tid, tm) = candidates[0];
        // Multiple catalog identities with identical evidence also require review.
        if catalog.iter().filter(|(_, m)| strong(m, tm)).count() != 1 {
            continue;
        }
        db.execute("INSERT OR IGNORE INTO audio_matches(catalog_id,track_id,method) SELECT ?1,?2,'automatic' WHERE NOT EXISTS(SELECT 1 FROM match_suppressions WHERE catalog_id=?1)",params![cid,tid]).map_err(|e|e.to_string())?;
    }
    Ok(())
}
#[derive(Serialize)]
pub(crate) struct Candidate {
    pub track_id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub reason: String,
    pub available: bool,
}
#[derive(Serialize)]
pub(crate) struct Review {
    pub catalog_id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_id: Option<i64>,
    pub method: Option<String>,
    pub available: bool,
    pub candidates: Vec<Candidate>,
}
pub(crate) fn review_at(root: &Path, catalog_id: Option<i64>) -> Result<Vec<Review>, String> {
    let db = open_catalog(&app_root(root)?)?;
    let locals = local_metadata(&db)?;
    let mut reviews = Vec::new();
    for (id, m) in catalog_metadata(&db)?
        .into_iter()
        .filter(|(id, _)| catalog_id.is_none_or(|wanted| wanted == *id))
    {
        let link = db
            .query_row(
                "SELECT track_id,method FROM audio_matches WHERE catalog_id=?1",
                [id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let candidates = locals
            .iter()
            .filter(|(_, lm)| {
                same(&m.title, &lm.title)
                    || (isrc(&m.isrc).is_some() && isrc(&m.isrc) == isrc(&lm.isrc))
            })
            .map(|(tid, lm)| Candidate {
                track_id: *tid,
                title: lm.title.clone().unwrap_or_else(|| "Untitled audio".into()),
                artist: lm.artist.clone(),
                album: lm.album.clone(),
                reason: if strong(&m, lm) {
                    "ISRC and complete metadata agree; review competing identities"
                } else {
                    "Partial evidence only; check recording/version"
                }
                .into(),
                available: resolve_track_audio_path_at(root, *tid).is_ok(),
            })
            .collect();
        let available = link
            .as_ref()
            .is_some_and(|(tid, _)| resolve_track_audio_path_at(root, *tid).is_ok());
        reviews.push(Review {
            catalog_id: id,
            title: m.title.unwrap_or_default(),
            artist: m.artist,
            album: m.album,
            track_id: link.as_ref().map(|v| v.0),
            method: link.map(|v| v.1),
            available,
            candidates,
        });
    }
    Ok(reviews)
}
pub(crate) fn set_match_at(
    root: &Path,
    catalog_id: i64,
    track_id: Option<i64>,
) -> Result<(), String> {
    if catalog_id <= 0 {
        return Err("Invalid catalog ID".into());
    }
    if let Some(id) = track_id {
        resolve_track_audio_path_at(root, id)?;
    }
    let mut db = open_catalog(&app_root(root)?)?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM catalog_tracks WHERE id=?1)",
            [catalog_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err("Catalog track not found".into());
    }
    tx.execute(
        "DELETE FROM audio_matches WHERE catalog_id=?1",
        [catalog_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT OR IGNORE INTO match_suppressions(catalog_id) VALUES(?1)",
        [catalog_id],
    )
    .map_err(|e| e.to_string())?;
    if let Some(id) = track_id {
        tx.execute(
            "INSERT INTO audio_matches(catalog_id,track_id,method) VALUES(?1,?2,'manual')",
            params![catalog_id, id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn list_audio_match_review(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    catalog_id: Option<i64>,
) -> Result<Vec<Review>, String> {
    use tauri::Manager;
    let root = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let lock = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().map_err(|_| "Catalog busy")?;
        review_at(&root, catalog_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn set_audio_match(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::IosCatalogLock>,
    catalog_id: i64,
    track_id: Option<i64>,
) -> Result<(), String> {
    use tauri::Manager;
    let root = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let lock = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock.lock().map_err(|_| "Catalog busy")?;
        set_match_at(&root, catalog_id, track_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    use lofty::{picture::{MimeType, Picture, PictureType}, tag::{Tag, TagExt, TagType}};

    fn synthetic_wav(path: &Path) {
        let samples = vec![128_u8; 96];
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
        std::fs::write(path, bytes).unwrap();
    }
    fn metadata(title: &str) -> Metadata {
        Metadata {
            title: Some(title.into()),
            artist: Some("Artist".into()),
            album: Some("Album".into()),
            isrc: Some("USAAA2600001".into()),
            error: None,
        }
    }
    #[test]
    fn versions_missing_tags_and_invalid_identifiers_never_auto_match() {
        let a = metadata("Song");
        assert!(strong(&a, &metadata(" song ")));
        for title in [
            "Song (Live)",
            "Song - Remastered",
            "Song (Acoustic)",
            "Song (Remix)",
        ] {
            assert!(!strong(&a, &metadata(title)));
        }
        let mut b = a.clone();
        b.isrc = None;
        assert!(!strong(&a, &b));
        b.isrc = Some("bad".into());
        assert!(!strong(&b, &b));
        b = a.clone();
        b.artist = Some("Cover artist".into());
        assert!(!strong(&a, &b));
    }

    #[test]
    fn reads_bounded_embedded_id3_artwork_and_returns_none_when_absent() {
        let base = std::env::temp_dir().join(format!("mypod-art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("cover.wav");
        synthetic_wav(&path);
        let expected = b"\x89PNG\r\n\x1a\nsynthetic-cover".to_vec();
        let mut tag = Tag::new(TagType::Id3v2);
        tag.push_picture(Picture::new_unchecked(
            PictureType::CoverFront,
            Some(MimeType::Png),
            None,
            expected.clone(),
        ));
        tag.save_to_path(&path, lofty::config::WriteOptions::default()).unwrap();
        assert_eq!(read_cover_art(&path), Some((expected, "image/png".into())));

        let plain = base.join("plain.wav");
        synthetic_wav(&plain);
        assert_eq!(read_cover_art(&plain), None);
        let _ = std::fs::remove_dir_all(base);
    }
    #[test]
    fn embedded_tags_link_all_memberships_manual_unlink_and_missing_audio() {
        use crate::{
            ios_catalog::register_import_at, ios_playlists::list_entries_at,
            ios_spotify_import::import_at, ios_spotify_preview::parse_spotify_csv,
        };
        use lofty::tag::{Tag, TagExt, TagType};
        let base = std::env::temp_dir().join(format!("mypod-tagged-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let root = app_root(&base).unwrap();
        let staging = root.join("Importing");
        std::fs::create_dir_all(&staging).unwrap();
        let token = "abcde000000000000000000000000000";
        let path = staging.join(format!("{token}.part"));
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&8036u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&vec![128u8; 8000]);
        std::fs::write(&path, bytes).unwrap();
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Song".into());
        tag.set_artist("Artist".into());
        tag.set_album("Album".into());
        tag.insert_text(ItemKey::Isrc, "USAAA2600001".into());
        tag.save_to_path(&path, lofty::config::WriteOptions::default())
            .unwrap();
        assert!(strong(&read_metadata(&path), &metadata("Song")));
        let preview=parse_spotify_csv(b"Track name,Artist name,Album,Playlist name,Type,ISRC,Spotify - id\nSong,Artist,Album,One,Playlist,USAAA2600001,id\nSong,Artist,Album,Two,Playlist,USAAA2600001,id\n");
        import_at(&base, &preview).unwrap();
        let imported = register_import_at(&base, token, "Different filename.wav", "wav").unwrap();
        let lists = crate::ios_playlists::list_playlists_at(&base).unwrap();
        for p in &lists {
            assert_eq!(
                list_entries_at(&base, p.id).unwrap()[0].track_id,
                Some(imported.track.id)
            );
        }
        let page = crate::ios_catalog::list_tracks_at(&base, 0).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].title, "Song");
        assert_eq!(page.items[0].artist.as_deref(), Some("Artist"));
        for p in &lists {
            assert_eq!(list_entries_at(&base, p.id).unwrap()[0].title, "Song");
        }
        let review = review_at(&base, None).unwrap();
        assert_eq!(review.len(), 1);
        assert_eq!(review[0].method.as_deref(), Some("automatic"));
        set_match_at(&base, review[0].catalog_id, None).unwrap();
        for p in &lists {
            assert_eq!(list_entries_at(&base, p.id).unwrap()[0].track_id, None);
        }
        set_match_at(&base, review[0].catalog_id, Some(imported.track.id)).unwrap();
        assert_eq!(
            review_at(&base, None).unwrap()[0].method.as_deref(),
            Some("manual")
        );
        std::fs::remove_file(resolve_track_audio_path_at(&base, imported.track.id).unwrap())
            .unwrap();
        assert!(!review_at(&base, None).unwrap()[0].available);
        for p in &lists {
            assert_eq!(list_entries_at(&base, p.id).unwrap()[0].track_id, None);
        }
        assert!(resolve_track_audio_path_at(&base, imported.track.id).is_err());
        std::fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn exact_and_ambiguous_candidates_and_suppression() {
        let root = std::env::temp_dir().join(format!("mypod-matches-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let db = open_catalog(&app_root(&root).unwrap()).unwrap();
        db.execute_batch("INSERT INTO imported_tracks(title,original_name,extension,relative_path,content_sha256,file_size,imported_at) VALUES('Song','Song.wav','wav','Media/test.wav','synthetic',1,1);
          INSERT INTO catalog_tracks(title,artist,album,isrc) VALUES('Song','Artist','Album','USAAA2600001');").unwrap();
        std::fs::create_dir_all(app_root(&root).unwrap().join("Media")).unwrap();
        std::fs::write(
            app_root(&root).unwrap().join("Media/test.wav"),
            b"synthetic",
        )
        .unwrap();
        save_metadata(&db, 1, &metadata("Song")).unwrap();
        auto_match(&db, &app_root(&root).unwrap()).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM audio_matches", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        // A second available tagged file removes the previous automatic choice.
        db.execute_batch("INSERT INTO imported_tracks(title,original_name,extension,relative_path,content_sha256,file_size,imported_at) VALUES('Song','Other.wav','wav','Media/other.wav','second',1,1);").unwrap();
        std::fs::write(
            app_root(&root).unwrap().join("Media/other.wav"),
            b"synthetic",
        )
        .unwrap();
        save_metadata(&db, 2, &metadata("Song")).unwrap();
        auto_match(&db, &app_root(&root).unwrap()).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM audio_matches", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        db.execute_batch(
            "DELETE FROM audio_metadata WHERE track_id=2; DELETE FROM imported_tracks WHERE id=2;",
        )
        .unwrap();
        auto_match(&db, &app_root(&root).unwrap()).unwrap();
        set_match_at(&root, 1, None).unwrap();
        auto_match(&db, &app_root(&root).unwrap()).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM audio_matches", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        db.execute_batch("DELETE FROM match_suppressions; INSERT INTO catalog_tracks(title,artist,album,isrc) VALUES('Song','Artist','Album','USAAA2600001');").unwrap();
        auto_match(&db, &app_root(&root).unwrap()).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM audio_matches", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        std::fs::remove_file(app_root(&root).unwrap().join("Media/test.wav")).unwrap();
        assert!(set_match_at(&root, 1, Some(1)).is_err()); // missing audio is never linkable
        std::fs::remove_dir_all(root).unwrap();
    }
}
