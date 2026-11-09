use block2::RcBlock;
use objc2::{rc::Retained, runtime::{AnyObject, AnyClass, ProtocolObject}, MainThreadMarker};
use objc2_av_foundation::{
    AVPlayer, AVPlayerItemStatus, AVPlayerStatus, AVPlayerTimeControlStatus,
};
use objc2_avf_audio::{
    AVAudioSession, AVAudioSessionCategoryPlayback, AVAudioSessionInterruptionNotification,
    AVAudioSessionRouteChangeNotification,
};
use objc2_core_media::CMTime;
use objc2_foundation::{
    NSData, NSMutableDictionary, NSNumber, NSCopying, NSNotification,
    NSNotificationCenter, NSOperationQueue, NSObjectProtocol, NSString, NSURL,
};
use objc2_media_player::{
    MPMediaItemArtwork, MPNowPlayingInfoCenter, MPRemoteCommand, MPRemoteCommandCenter,
    MPRemoteCommandEvent, MPRemoteCommandHandlerStatus, MPChangePlaybackPositionCommandEvent,
};
use objc2_ui_kit::UIImage;
use serde::Serialize;
use std::collections::VecDeque;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};

fn make_system_artwork(bytes: &[u8]) -> Option<Retained<AnyObject>> {
    let data = NSData::with_bytes(bytes);
    let image = UIImage::imageWithData(&data)?;
    let class = AnyClass::get(c"MPMediaItemArtwork")?;
    // objc2-media-player 0.3.2 does not expose MPMediaItemArtwork's UIKit
    // initializer on iOS, although Apple's iOS SDK does. Keep this one call
    // narrowly typed and use the supported image initializer.
    let allocated: objc2::rc::Allocated<MPMediaItemArtwork> = unsafe { objc2::msg_send![class, alloc] };
    let artwork: Option<Retained<MPMediaItemArtwork>> = unsafe {
        objc2::msg_send![allocated, initWithImage: &*image]
    };
    artwork.map(|artwork| artwork.into_super().into_super())
}

fn load_track_metadata(app_local_data: &std::path::Path, track_id: i64) -> Result<CurrentMetadata, String> {
    let track = crate::ios_catalog::get_track_at(app_local_data, track_id)?;
    let artwork = crate::ios_catalog::artwork_for_track_at(app_local_data, track_id)
        .ok()
        .flatten()
        .and_then(|payload| make_system_artwork(&payload.bytes));
    Ok(CurrentMetadata { title: track.title, artist: track.artist, album: track.album, artwork })
}

fn set_now_playing_value(
    info: &NSMutableDictionary<NSString, AnyObject>,
    key: &str,
    value: &AnyObject,
) {
    let key = NSString::from_str(key);
    let key = ProtocolObject::<dyn NSCopying>::from_ref(&*key);
    unsafe { info.setObject_forKey(value, key) };
}

fn publish_now_playing(
    metadata: &CurrentMetadata,
    current_time: f64,
    duration: Option<f64>,
    playing: bool,
) {
    let info = NSMutableDictionary::<NSString, AnyObject>::new();
    let title = NSString::from_str(&metadata.title);
    set_now_playing_value(&info, "title", &title);
    if let Some(artist) = metadata.artist.as_deref() {
        let value = NSString::from_str(artist);
        set_now_playing_value(&info, "artist", &value);
    }
    if let Some(album) = metadata.album.as_deref() {
        let value = NSString::from_str(album);
        set_now_playing_value(&info, "albumTitle", &value);
    }
    if let Some(duration) = duration.filter(|value| value.is_finite() && *value > 0.0) {
        let value = NSNumber::numberWithDouble(duration);
        set_now_playing_value(&info, "playbackDuration", &value);
    }
    let elapsed = NSNumber::numberWithDouble(current_time.max(0.0));
    let rate = NSNumber::numberWithDouble(if playing { 1.0 } else { 0.0 });
    set_now_playing_value(&info, "elapsedPlaybackTime", &elapsed);
    set_now_playing_value(&info, "playbackRate", &rate);
    if let Some(artwork) = metadata.artwork.as_deref() {
        set_now_playing_value(&info, "artwork", artwork);
    }
    let center = unsafe { MPNowPlayingInfoCenter::defaultCenter() };
    unsafe { center.setNowPlayingInfo(Some(&info)) };
}

fn confirm_native_start(instance: &mut PlaybackInstance, track_id: i64) {
    if instance.last_started_track_id != Some(track_id) {
        if let Some(previous) = instance.last_started_track_id {
            instance.remote_history.push(previous);
            if instance.remote_history.len() > 100 {
                instance.remote_history.remove(0);
            }
        }
        instance.last_started_track_id = Some(track_id);
    }
    if instance.pending_queue_track_id == Some(track_id) {
        if instance.remote_queue.front() == Some(&track_id) {
            instance.remote_queue.pop_front();
        }
        instance.pending_queue_track_id = None;
    }
}

fn add_time_observer(
    app: tauri::AppHandle,
    instance: &mut PlaybackInstance,
    player: &Retained<AVPlayer>,
    track_id: i64,
) {
    let item = unsafe { player.currentItem() };
    let Some(item) = item else { return };
    let block = RcBlock::new(move |time: CMTime| {
        let current_time = unsafe { time.seconds() };
        if !current_time.is_finite() || current_time < 0.0 { return; }
        let duration = unsafe { item.duration().seconds() };
        let duration = (duration.is_finite() && duration > 0.0).then_some(duration);
        let state = app.state::<IosPlaybackState>();
        let (playing, metadata) = match state.0.lock() {
            Ok(mut instance) if instance.track_id == Some(track_id) => {
                let playing = instance.player.as_ref().is_some_and(|player| unsafe {
                    player.timeControlStatus() == AVPlayerTimeControlStatus::Playing
                });
                if playing { confirm_native_start(&mut instance, track_id); }
                (playing, instance.metadata.clone())
            }
            _ => return,
        };
        if let Some(metadata) = metadata {
            publish_now_playing(&metadata, current_time, duration, playing);
        }
        let _ = app.emit("local-playback-progress", NativePlaybackProgress {
            track_id, current_time, duration,
        });
    });
    let observer = unsafe {
        player.addPeriodicTimeObserverForInterval_queue_usingBlock(
            CMTime::with_seconds(0.25, 600), None, &block,
        )
    };
    instance.time_observer = Some(observer);
}

fn clear_time_observer(instance: &mut PlaybackInstance) {
    if let (Some(player), Some(observer)) = (instance.player.as_ref(), instance.time_observer.take()) {
        unsafe { player.removeTimeObserver(&observer) };
    }
}

fn pause_for_system_audio_event(app: tauri::AppHandle, label: &'static str) {
    let state = app.state::<IosPlaybackState>();
    let (metadata, current_time, duration) = match state.0.lock() {
        Ok(instance) => {
            let Some(player) = instance.player.as_ref() else { return };
            unsafe { player.pause() };
            let current_time = unsafe { player.currentTime().seconds() };
            let duration = unsafe { player.currentItem() }
                .and_then(|item| unsafe { item.duration().seconds() }.is_finite().then(|| unsafe { item.duration().seconds() }))
                .filter(|duration| *duration > 0.0);
            (instance.metadata.clone(), current_time.max(0.0), duration)
        }
        Err(_) => return,
    };
    if let Some(metadata) = metadata {
        publish_now_playing(&metadata, current_time, duration, false);
    }
    eprintln!("MyPod iOS playback: system audio event; playback paused ({label})");
    let _ = app.emit("local-playback-system-pause", ());
}

fn install_audio_observers(app: &tauri::AppHandle, instance: &mut PlaybackInstance) {
    if !instance.notification_observers.is_empty() { return; }
    let center = NSNotificationCenter::defaultCenter();
    let main_queue = NSOperationQueue::mainQueue();
    let interruption_app = app.clone();
    let interruption = RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
        // Do not auto-resume after calls or Siri; explicit user action avoids
        // unexpected audio restarting when an interruption ends.
        pause_for_system_audio_event(interruption_app.clone(), "audio interruption");
    });
    if let Some(name) = unsafe { AVAudioSessionInterruptionNotification } {
        let token = unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, Some(&main_queue), &interruption) };
        instance.notification_observers.push(token);
    }
    let route_app = app.clone();
    let route = RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
        // Pause conservatively on route changes (including headphone removal).
        pause_for_system_audio_event(route_app.clone(), "audio route change");
    });
    if let Some(name) = unsafe { AVAudioSessionRouteChangeNotification } {
        let token = unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, Some(&main_queue), &route) };
        instance.notification_observers.push(token);
    }
}

#[derive(Clone, Copy)]
enum RemoteAction { Play, Pause, Toggle, Next, Previous, Seek }

fn remote_next(app: tauri::AppHandle) -> MPRemoteCommandHandlerStatus {
    let state = app.state::<IosPlaybackState>();
    let track_id = match state.0.lock() {
        Ok(mut instance) => {
            let Some(track_id) = instance.remote_queue.front().copied() else {
                return MPRemoteCommandHandlerStatus::NoSuchContent;
            };
            instance.pending_queue_track_id = Some(track_id);
            track_id
        }
        Err(_) => return MPRemoteCommandHandlerStatus::CommandFailed,
    };
    match play_local_track(app.clone(), state, track_id, true) {
        Ok(()) => MPRemoteCommandHandlerStatus::Success,
        Err(_) => {
            if let Ok(mut instance) = app.state::<IosPlaybackState>().0.lock() {
                instance.pending_queue_track_id = None;
            }
            MPRemoteCommandHandlerStatus::CommandFailed
        }
    }
}

fn remote_previous(app: tauri::AppHandle) -> MPRemoteCommandHandlerStatus {
    let state = app.state::<IosPlaybackState>();
    let (track_id, seek_to_start, from_history) = match state.0.lock() {
        Ok(mut instance) => {
            let Some(player) = instance.player.as_ref() else {
                return MPRemoteCommandHandlerStatus::NoActionableNowPlayingItem;
            };
            let current = unsafe { player.currentTime().seconds() };
            if current > 3.0 {
                (instance.track_id.unwrap_or_default(), true, false)
            } else {
                let previous = instance.remote_history.pop();
                let target = previous.or(instance.track_id);
                let Some(target) = target else { return MPRemoteCommandHandlerStatus::NoActionableNowPlayingItem; };
                (target, false, previous.is_some())
            }
        }
        Err(_) => return MPRemoteCommandHandlerStatus::CommandFailed,
    };
    if seek_to_start {
        let _ = seek_local_playback(state, 0.0);
        return MPRemoteCommandHandlerStatus::Success;
    }
    match play_local_track(app.clone(), state, track_id, true) {
        Ok(()) => MPRemoteCommandHandlerStatus::Success,
        Err(_) => {
            if from_history {
                if let Ok(mut instance) = app.state::<IosPlaybackState>().0.lock() {
                instance.remote_history.push(track_id);
                }
            }
            MPRemoteCommandHandlerStatus::CommandFailed
        }
    }
}

fn handle_remote_action(
    app: tauri::AppHandle,
    action: RemoteAction,
    event: std::ptr::NonNull<MPRemoteCommandEvent>,
) -> MPRemoteCommandHandlerStatus {
    if matches!(action, RemoteAction::Next) { return remote_next(app); }
    if matches!(action, RemoteAction::Previous) { return remote_previous(app); }
    let state = app.state::<IosPlaybackState>();
    let guard = match state.0.lock() {
        Ok(guard) => guard,
        Err(_) => return MPRemoteCommandHandlerStatus::CommandFailed,
    };
    let Some(player) = guard.player.as_ref() else {
        return MPRemoteCommandHandlerStatus::NoActionableNowPlayingItem;
    };
    match action {
        RemoteAction::Play => unsafe { player.play() },
        RemoteAction::Pause => unsafe { player.pause() },
        RemoteAction::Toggle => {
            if unsafe { player.timeControlStatus() == AVPlayerTimeControlStatus::Playing } {
                unsafe { player.pause() };
            } else {
                unsafe { player.play() };
            }
        }
        RemoteAction::Seek => {
            let event = unsafe { &*event.as_ptr().cast::<MPChangePlaybackPositionCommandEvent>() };
            let seconds = unsafe { event.positionTime() };
            if !seconds.is_finite() || seconds < 0.0 { return MPRemoteCommandHandlerStatus::CommandFailed; }
            let time = unsafe { CMTime::with_seconds(seconds, 600) };
            unsafe { player.seekToTime(time) };
        }
        RemoteAction::Next | RemoteAction::Previous => unreachable!(),
    }
    let current_time = unsafe { player.currentTime().seconds() }.max(0.0);
    let duration = unsafe { player.currentItem() }
        .map(|item| unsafe { item.duration().seconds() })
        .filter(|duration| duration.is_finite() && *duration > 0.0);
    let playing = unsafe { player.timeControlStatus() == AVPlayerTimeControlStatus::Playing };
    let metadata = guard.metadata.clone();
    drop(guard);
    if let Some(metadata) = metadata { publish_now_playing(&metadata, current_time, duration, playing); }
    MPRemoteCommandHandlerStatus::Success
}

fn add_remote_target(
    command: &MPRemoteCommand,
    app: tauri::AppHandle,
    action: RemoteAction,
) -> Retained<AnyObject> {
    let block = RcBlock::new(move |event: std::ptr::NonNull<MPRemoteCommandEvent>| {
        handle_remote_action(app.clone(), action, event)
    });
    unsafe { command.addTargetWithHandler(&block) }
}

fn install_remote_commands(app: &tauri::AppHandle, instance: &mut PlaybackInstance) {
    if instance.controls_installed { return; }
    let center = unsafe { MPRemoteCommandCenter::sharedCommandCenter() };
    unsafe {
        let play = center.playCommand();
        let pause = center.pauseCommand();
        let toggle = center.togglePlayPauseCommand();
        let next = center.nextTrackCommand();
        let previous = center.previousTrackCommand();
        let seek = center.changePlaybackPositionCommand();
        play.setEnabled(true);
        pause.setEnabled(true);
        toggle.setEnabled(true);
        next.setEnabled(!instance.remote_queue.is_empty());
        previous.setEnabled(true);
        seek.setEnabled(true);
        instance.remote_targets.push(add_remote_target(&play, app.clone(), RemoteAction::Play));
        instance.remote_targets.push(add_remote_target(&pause, app.clone(), RemoteAction::Pause));
        instance.remote_targets.push(add_remote_target(&toggle, app.clone(), RemoteAction::Toggle));
        instance.remote_targets.push(add_remote_target(&next, app.clone(), RemoteAction::Next));
        instance.remote_targets.push(add_remote_target(&previous, app.clone(), RemoteAction::Previous));
        instance.remote_targets.push(add_remote_target(&seek, app.clone(), RemoteAction::Seek));
    }
    instance.controls_installed = true;
}

#[derive(Clone)]
struct CurrentMetadata {
    title: String,
    artist: Option<String>,
    album: Option<String>,
    artwork: Option<Retained<AnyObject>>,
}

unsafe impl Send for CurrentMetadata {}

struct PlaybackInstance {
    track_id: Option<i64>,
    player: Option<objc2::rc::Retained<AVPlayer>>,
    time_observer: Option<Retained<AnyObject>>,
    notification_observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    remote_targets: Vec<Retained<AnyObject>>,
    remote_queue: VecDeque<i64>,
    remote_history: Vec<i64>,
    last_started_track_id: Option<i64>,
    pending_queue_track_id: Option<i64>,
    metadata: Option<CurrentMetadata>,
    controls_installed: bool,
    failure_reported: bool,
    last_reported_state: Option<String>,
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
            time_observer: None,
            notification_observers: Vec::new(),
            remote_targets: Vec::new(),
            remote_queue: VecDeque::new(),
            remote_history: Vec::new(),
            last_started_track_id: None,
            pending_queue_track_id: None,
            metadata: None,
            controls_installed: false,
            failure_reported: false,
            last_reported_state: None,
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
    pub ready: bool,
    pub player_status: &'static str,
    pub item_status: &'static str,
    pub time_control_status: &'static str,
    pub failure_code: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct NativePlaybackProgress {
    track_id: i64,
    current_time: f64,
    duration: Option<f64>,
}

fn main_thread() -> Result<MainThreadMarker, String> {
    MainThreadMarker::new()
        .ok_or_else(|| "iOS playback must be controlled on the main thread".to_string())
}

fn resolve_audio_path(app_local_data: PathBuf, track_id: i64) -> Result<PathBuf, String> {
    crate::ios_catalog::resolve_track_audio_path_at(&app_local_data, track_id)
}

fn configure_audio_session() -> Result<(), String> {
    let session = unsafe { AVAudioSession::sharedInstance() };
    let category = unsafe { AVAudioSessionCategoryPlayback }
        .ok_or_else(|| "iOS playback category is unavailable".to_string())?;
    unsafe {
        session
            .setCategory_error(category)
            .map_err(|_| "Could not configure the iOS playback session".to_string())?;
        session
            .setActive_error(true)
            .map_err(|_| "Could not activate the iOS playback session".to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn play_local_track(
    app: tauri::AppHandle,
    state: State<'_, IosPlaybackState>,
    track_id: i64,
    reload: bool,
) -> Result<(), String> {
    let marker = main_thread()?;
    eprintln!("MyPod iOS playback: play tapped; native command received");
    let app_local_data = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "App-private storage is unavailable".to_string())?;
    let path = resolve_audio_path(app_local_data.clone(), track_id).map_err(|_| {
        eprintln!("MyPod iOS playback: track lookup or managed-file resolution failed");
        "The imported audio is missing or unavailable".to_string()
    })?;
    File::open(&path).map_err(|_| {
        eprintln!("MyPod iOS playback: track found but managed audio is not readable");
        "The imported audio cannot be read by MyPod".to_string()
    })?;
    eprintln!("MyPod iOS playback: track found; managed audio is readable");
    configure_audio_session().map_err(|error| {
        eprintln!("MyPod iOS playback: audio session configuration failed");
        error
    })?;
    let metadata = if reload {
        Some(load_track_metadata(&app_local_data, track_id).map_err(|_| {
            eprintln!("MyPod iOS playback: track metadata unavailable for lock-screen display");
            "Track metadata is unavailable".to_string()
        })?)
    } else {
        None
    };
    let mut instance = state.0.lock().map_err(|_| "Playback is busy".to_string())?;
    install_audio_observers(&app, &mut instance);
    install_remote_commands(&app, &mut instance);

    if reload || instance.track_id != Some(track_id) || instance.player.is_none() {
        clear_time_observer(&mut instance);
        if let Some(previous) = instance.player.as_ref() {
            unsafe { previous.pause() };
        }
        let path = path.to_str().ok_or_else(|| {
            eprintln!("MyPod iOS playback: audio URL creation failed");
            "The imported audio location is unavailable".to_string()
        })?;
        let path = NSString::from_str(path);
        let url = NSURL::fileURLWithPath(&path);
        let player = unsafe { AVPlayer::playerWithURL(&url, marker) };
        instance.player = Some(player);
        instance.track_id = Some(track_id);
        if instance.pending_queue_track_id != Some(track_id) {
            instance.pending_queue_track_id = None;
        }
        if let Some(metadata) = metadata { instance.metadata = Some(metadata); }
        instance.failure_reported = false;
        instance.last_reported_state = None;
        if let Some(player) = instance.player.clone() {
            add_time_observer(app.clone(), &mut instance, &player, track_id);
        }
        if let (Some(metadata), Some(player)) = (instance.metadata.as_ref(), instance.player.as_ref()) {
            let current = unsafe { player.currentTime().seconds() }.max(0.0);
            let duration = unsafe { player.currentItem() }
                .map(|item| unsafe { item.duration().seconds() })
                .filter(|duration| duration.is_finite() && *duration > 0.0);
            publish_now_playing(metadata, current, duration, false);
        }
        eprintln!("MyPod iOS playback: AVPlayer initialized");
    }

    let player = instance
        .player
        .as_ref()
        .ok_or_else(|| "Could not create the audio player".to_string())?;
    unsafe { player.play() };
    let current_time = unsafe { player.currentTime().seconds() }.max(0.0);
    let duration = unsafe { player.currentItem() }
        .map(|item| unsafe { item.duration().seconds() })
        .filter(|duration| duration.is_finite() && *duration > 0.0);
    if let Some(metadata) = instance.metadata.as_ref() {
        publish_now_playing(metadata, current_time, duration, false);
    }
    eprintln!("MyPod iOS playback: AVPlayer play requested; awaiting native state");
    Ok(())
}

#[tauri::command]
pub fn pause_local_playback(state: State<'_, IosPlaybackState>) -> Result<(), String> {
    let _marker = main_thread()?;
    let instance = state.0.lock().map_err(|_| "Playback is busy".to_string())?;
    if let Some(player) = instance.player.as_ref() {
        unsafe { player.pause() };
        let current_time = unsafe { player.currentTime().seconds() }.max(0.0);
        let duration = unsafe { player.currentItem() }
            .map(|item| unsafe { item.duration().seconds() })
            .filter(|duration| duration.is_finite() && *duration > 0.0);
        if let Some(metadata) = instance.metadata.as_ref() {
            publish_now_playing(metadata, current_time, duration, false);
        }
        eprintln!("MyPod iOS playback: AVPlayer pause requested");
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
pub fn set_local_playback_queue(
    state: State<'_, IosPlaybackState>,
    track_ids: Vec<i64>,
) -> Result<(), String> {
    if track_ids.iter().any(|id| *id <= 0) {
        return Err("Queue entries must be valid library track IDs".to_string());
    }
    let mut instance = state.0.lock().map_err(|_| "Playback is busy".to_string())?;
    instance.remote_queue = track_ids.into();
    if instance.controls_installed {
        let center = unsafe { MPRemoteCommandCenter::sharedCommandCenter() };
        unsafe { center.nextTrackCommand().setEnabled(!instance.remote_queue.is_empty()) };
    }
    Ok(())
}

#[tauri::command]
pub fn local_playback_state(
    state: State<'_, IosPlaybackState>,
) -> Result<NativePlaybackSnapshot, String> {
    let _marker = main_thread()?;
    let mut instance = state.0.lock().map_err(|_| "Playback is busy".to_string())?;
    let Some(player) = instance.player.as_ref() else {
        return Ok(NativePlaybackSnapshot {
            track_id: None,
            current_time: 0.0,
            duration: None,
            playing: false,
            loading: false,
            ended: false,
            error: None,
            ready: false,
            player_status: "unavailable",
            item_status: "unavailable",
            time_control_status: "paused",
            failure_code: None,
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
    let player_status_name = if player_status == AVPlayerStatus::ReadyToPlay {
        "ready"
    } else if player_status == AVPlayerStatus::Failed {
        "failed"
    } else {
        "unknown"
    };
    let item_status = item.as_ref().map(|item| unsafe { item.status() });
    let item_status_name = match item_status {
        Some(AVPlayerItemStatus::ReadyToPlay) => "ready",
        Some(AVPlayerItemStatus::Failed) => "failed",
        Some(_) => "unknown",
        None => "unavailable",
    };
    let item_failed = item
        .as_ref()
        .is_some_and(|_| item_status == Some(AVPlayerItemStatus::Failed));
    let media_failed = player_status == AVPlayerStatus::Failed || item_failed;
    let should_report_failure = media_failed && !instance.failure_reported;
    let error = if media_failed {
        Some("iOS could not load this audio file. Try another file or re-import it.".to_string())
    } else {
        None
    };
    let time_status = unsafe { player.timeControlStatus() };
    let time_control_status_name = if time_status == AVPlayerTimeControlStatus::Playing {
        "playing"
    } else if time_status == AVPlayerTimeControlStatus::WaitingToPlayAtSpecifiedRate {
        "waiting"
    } else {
        "paused"
    };
    let playing = time_status == AVPlayerTimeControlStatus::Playing;
    let ended = !playing
        && duration.is_some_and(|duration| current_time + 0.1 >= duration)
        && error.is_none();
    let loading = !playing
        && !ended
        && error.is_none()
        && (player_status == AVPlayerStatus::Unknown
            || item_status.is_none_or(|status| status == AVPlayerItemStatus::Unknown)
            || time_status == AVPlayerTimeControlStatus::WaitingToPlayAtSpecifiedRate);

    let failure_code = item
        .as_ref()
        .and_then(|item| unsafe { item.error() })
        .or_else(|| unsafe { player.error() })
        .map(|error| error.code() as i64);

    let ready = player_status == AVPlayerStatus::ReadyToPlay
        && item_status == Some(AVPlayerItemStatus::ReadyToPlay);
    let diagnostic_state = format!(
        "player={player_status_name},item={item_status_name},time={time_control_status_name},ready={ready},duration={}",
        duration.is_some()
    );
    if instance.last_reported_state.as_deref() != Some(&diagnostic_state) {
        eprintln!("MyPod iOS playback: state {diagnostic_state}");
        instance.last_reported_state = Some(diagnostic_state);
    }

    if should_report_failure {
        eprintln!(
            "MyPod iOS playback: AVPlayer reported a media failure; code={}",
            failure_code.map_or_else(|| "unavailable".to_string(), |code| code.to_string())
        );
        instance.failure_reported = true;
    }

    Ok(NativePlaybackSnapshot {
        track_id: instance.track_id,
        current_time,
        duration,
        playing,
        loading,
        ended,
        error,
        ready,
        player_status: player_status_name,
        item_status: item_status_name,
        time_control_status: time_control_status_name,
        failure_code,
    })
}
