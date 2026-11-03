#[cfg(any(target_os = "ios", test))]
mod ios_catalog;

#[cfg(any(target_os = "ios", test))]
mod ios_playlists;

#[cfg(any(target_os = "ios", test))]
mod ios_spotify_import;

#[cfg(any(target_os = "ios", test))]
mod ios_audio_matching;

#[cfg(target_os = "ios")]
mod ios_playback;

#[cfg(target_os = "ios")]
mod ios_diagnostics;

#[cfg(any(target_os = "ios", test))]
mod ios_spotify_preview;

#[cfg(any(target_os = "ios", test))]
mod ios_backup;

#[cfg(target_os = "ios")]
use std::sync::{Arc, Mutex};

#[cfg(target_os = "ios")]
#[derive(Clone, Default)]
pub(crate) struct IosCatalogLock(Arc<Mutex<()>>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[cfg(not(test))]
pub fn run() {
    let builder = tauri::Builder::default();

    #[cfg(target_os = "ios")]
    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(IosCatalogLock::default())
        .manage(ios_spotify_import::PendingImport::default())
        .manage(ios_playback::IosPlaybackState::default())
        .invoke_handler(tauri::generate_handler![
            ios_catalog::list_tracks,
            ios_catalog::get_track,
            ios_catalog::get_track_artwork,
            ios_catalog::register_imported_track,
            ios_playlists::list_playlists,
            ios_playlists::create_playlist,
            ios_playlists::rename_playlist,
            ios_playlists::delete_playlist,
            ios_playlists::list_playlist_entries,
            ios_playlists::add_track_to_playlist,
            ios_playlists::remove_playlist_entry,
            ios_playlists::reorder_playlist_entries,
            ios_playback::play_local_track,
            ios_playback::pause_local_playback,
            ios_playback::seek_local_playback,
            ios_playback::local_playback_state,
            ios_audio_matching::list_audio_match_review,
            ios_audio_matching::set_audio_match,
            ios_spotify_import::confirm_spotify_import,
            ios_spotify_preview::preview_spotify_csv,
            ios_diagnostics::ios_runtime_diagnostics,
            ios_backup::create_local_backup,
            ios_backup::inspect_local_backup,
            ios_backup::restore_local_backup
        ]);

    #[cfg(not(target_os = "ios"))]
    let builder = builder.plugin(tauri_plugin_dialog::init());

    builder
        .run(tauri::generate_context!())
        .expect("error while running myPod app");
}
