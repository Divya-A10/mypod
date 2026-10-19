use objc2::MainThreadMarker;
use objc2_av_foundation::{
    AVPlayer, AVPlayerItemStatus, AVPlayerStatus, AVPlayerTimeControlStatus,
};
use objc2_core_media::CMTime;
use objc2_foundation::{NSString, NSURL};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, State};

struct PlaybackInstance {
    track_id: Option<i64>,
    player: Option<objc2::rc::Retained<AVPlayer>>,
}

// AVPlayer is only accessed by commands after MainThreadMarker confirms the
// calling thread is the iOS main thread. The mutex also serializes command access.
unsafe impl Send for PlaybackInstance {}

pub struct IosPlaybackState(Mutex<PlaybackInstance>);

impl Default for IosPlaybackState {
    fn default() -> Self {
        Self(Mutex::new(PlaybackInstance {
            track_id: None,
            player: None,
        }))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NativePlaybackSnapshot {
    pub track_id: Option<i64>,
    pub current_time: f64,
    pub duration: Option<f64>,
    pub playing: bool,
    pub loading: bool,
    pub ended: bool,
    pub error: Option<String>,
}

fn main_thread() -> Result<MainThreadMarker, String> {
    MainThreadMarker::new()
        .ok_or_else(|| "iOS playback must be controlled on the main thread".to_string())
}

fn resolve_audio_path(app_local_data: PathBuf, track_id: i64) -> Result<PathBuf, String> {
    crate::ios_catalog::resolve_track_audio_path_at(&app_local_data, track_id)
}

#[tauri::command]
pub fn play_local_track(
    app: tauri::AppHandle,
    state: State<'_, IosPlaybackState>,
    track_id: i64,
) -> Result<(), String> {
    let marker = main_thread()?;
    let app_local_data = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let path = resolve_audio_path(app_local_data, track_id)?;
    let mut instance = state.0.lock().map_err(|_| "Playback is busy".to_string())?;

    if instance.track_id != Some(track_id) || instance.player.is_none() {
        let path = path
            .to_str()
            .ok_or_else(|| "Track path is not valid UTF-8".to_string())?;
        let path = NSString::from_str(path);
        let url = NSURL::fileURLWithPath(&path);
        let player = unsafe { AVPlayer::playerWithURL(&url, marker) };
        instance.player = Some(player);
        instance.track_id = Some(track_id);
    }

    let player = instance
        .player
        .as_ref()
        .ok_or_else(|| "Could not create the audio player".to_string())?;
    unsafe { player.play() };
    Ok(())
}

#[tauri::command]
pub fn pause_local_playback(state: State<'_, IosPlaybackState>) -> Result<(), String> {
    let _marker = main_thread()?;
    let instance = state.0.lock().map_err(|_| "Playback is busy".to_string())?;
    if let Some(player) = instance.player.as_ref() {
        unsafe { player.pause() };
    }
    Ok(())
}

#[tauri::command]
pub fn seek_local_playback(state: State<'_, IosPlaybackState>, seconds: f64) -> Result<(), String> {
    let _marker = main_thread()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err("A valid seek position is required".to_string());
    }
    let instance = state.0.lock().map_err(|_| "Playback is busy".to_string())?;
    let player = instance
        .player
        .as_ref()
        .ok_or_else(|| "No track is loaded".to_string())?;
    let duration = unsafe { player.currentItem() }
        .map(|item| unsafe { item.duration().seconds() })
        .filter(|duration| duration.is_finite() && *duration >= 0.0)
        .unwrap_or(seconds);
    let target = seconds.min(duration);
    let time = unsafe { CMTime::with_seconds(target, 600) };
    unsafe { player.seekToTime(time) };
    Ok(())
}

#[tauri::command]
pub fn local_playback_state(
    state: State<'_, IosPlaybackState>,
) -> Result<NativePlaybackSnapshot, String> {
    let _marker = main_thread()?;
    let instance = state.0.lock().map_err(|_| "Playback is busy".to_string())?;
    let Some(player) = instance.player.as_ref() else {
        return Ok(NativePlaybackSnapshot {
            track_id: None,
            current_time: 0.0,
            duration: None,
            playing: false,
            loading: false,
            ended: false,
            error: None,
        });
    };

    let current_time = unsafe { player.currentTime().seconds() };
    let current_time = if current_time.is_finite() {
        current_time.max(0.0)
    } else {
        0.0
    };
    let item = unsafe { player.currentItem() };
    let duration = item
        .as_ref()
        .map(|item| unsafe { item.duration().seconds() })
        .filter(|duration| duration.is_finite() && *duration > 0.0);
    let player_status = unsafe { player.status() };
    let item_failed = item
        .as_ref()
        .is_some_and(|item| unsafe { item.status() == AVPlayerItemStatus::Failed });
    let error = if player_status == AVPlayerStatus::Failed || item_failed {
        Some("This audio file could not be decoded by AVFoundation.".to_string())
    } else {
        None
    };
    let time_status = unsafe { player.timeControlStatus() };
    let playing = time_status == AVPlayerTimeControlStatus::Playing;
    let ended = !playing
        && duration.is_some_and(|duration| current_time + 0.1 >= duration)
        && error.is_none();
    let loading = !playing
        && !ended
        && error.is_none()
        && (player_status == AVPlayerStatus::Unknown
            || time_status == AVPlayerTimeControlStatus::WaitingToPlayAtSpecifiedRate);

    Ok(NativePlaybackSnapshot {
        track_id: instance.track_id,
        current_time,
        duration,
        playing,
        loading,
        ended,
        error,
    })
}
