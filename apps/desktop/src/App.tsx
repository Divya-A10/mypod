import { useEffect, useMemo, useRef, useState, useSyncExternalStore, type CSSProperties, type FormEvent, type KeyboardEvent as ReactKeyboardEvent, type ReactNode, type ChangeEvent } from "react";
import myPodLogo from "./assets/mypod-logo.png";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { formatDuration, type Track } from "./api";
import { libraryAdapter } from "./library/libraryAdapter";
import { isIOSPlatform } from "./platform";
import type { AudioMatchReview, BackupScope, BackupSummary, IosRuntimeDiagnostics, Playlist, PlaylistEntry, SpotifyCsvPreview } from "./library/contracts";
import type { PlaybackSnapshot } from "./playback/PlaybackController";
import { playbackAvailable, playbackController } from "./playback/runtime";
import { loadTrackArtwork } from "./library/artwork";

type View = "Home" | "Search" | "Your Library" | "Liked Songs" | "Album";
const SPOTIFY_PREVIEW_PAGE_SIZE = 50;
type IconName = "home" | "search" | "library" | "heart" | "plus" | "arrow-left" | "arrow-right" | "play" | "pause" | "previous" | "next" | "volume" | "close" | "music" | "disc" | "artist" | "shuffle" | "repeat" | "menu" | "chevron" | "refresh" | "queue";

function Icon({ name, size = 20 }: { name: IconName; size?: number }) {
  const shared = { width: size, height: size, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.8, strokeLinecap: "round" as const, strokeLinejoin: "round" as const, "aria-hidden": true as const };
  const paths: Record<IconName, ReactNode> = {
    home: <><path d="m3 10 9-7 9 7" /><path d="M5 9v11h14V9M9 20v-6h6v6" /></>,
    search: <><circle cx="10.8" cy="10.8" r="6.8" /><path d="m16 16 5 5" /></>,
    library: <><path d="M4 4v16M8 4v16M12 6v14M17 5l3 14" /></>,
    heart: <path d="M20.8 8.7c0 5.2-8.8 10.3-8.8 10.3S3.2 13.9 3.2 8.7A4.6 4.6 0 0 1 12 6.4a4.6 4.6 0 0 1 8.8 2.3Z" />,
    plus: <><path d="M12 5v14M5 12h14" /></>,
    "arrow-left": <><path d="m15 18-6-6 6-6" /><path d="M20 12H9" /></>,
    "arrow-right": <><path d="m9 18 6-6-6-6" /><path d="M4 12h11" /></>,
    play: <path d="m8 5 12 7-12 7V5Z" fill="currentColor" stroke="none" />,
    pause: <><path d="M8 5h3v14H8zM15 5h3v14h-3z" fill="currentColor" stroke="none" /></>,
    previous: <><path d="M6 5v14M19 6l-10 6 10 6V6Z" fill="currentColor" stroke="none" /></>,
    next: <><path d="M18 5v14M5 6l10 6-10 6V6Z" fill="currentColor" stroke="none" /></>,
    volume: <><path d="M4 10v4h4l5 4V6l-5 4H4Z" /><path d="M17 9a5 5 0 0 1 0 6M19.5 6.5a9 9 0 0 1 0 11" /></>,
    close: <><path d="m6 6 12 12M18 6 6 18" /></>,
    music: <><path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" /></>,
    disc: <><circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="2.5" /></>,
    artist: <><circle cx="12" cy="8" r="4" /><path d="M4 21a8 8 0 0 1 16 0" /></>,
    shuffle: <><path d="m18 14 3 3-3 3M3 7h2c6 0 8 10 14 10h2M18 4l3 3-3 3M3 17h2c2 0 3.5-1.6 5-3.5M14 8c1.4-.7 2.7-1 5-1h2" /></>,
    repeat: <><path d="m17 2 4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4M21 13v2a3 3 0 0 1-3 3H3" /></>,
    queue: <><path d="M4 5h12M4 10h12M4 15h8M4 20h12" /><path d="m17 14 4 3-4 3v-6Z" /></>,
    menu: <><path d="M4 7h16M4 12h16M4 17h16" /></>,
    chevron: <path d="m7 10 5 5 5-5" />,
    refresh: <><path d="M20 7v5h-5M4 17v-5h5" /><path d="M5.5 9A7 7 0 0 1 18 6l2 1M4 17l2 1a7 7 0 0 0 12.5-3" /></>,
  };
  return <svg {...shared}>{paths[name]}</svg>;
}

function Art({ label, size = "card", trackId, eager = false }: { label: string; size?: "card" | "small" | "large"; trackId?: number | null; eager?: boolean }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [loadedArtwork, setLoadedArtwork] = useState<{ trackId: number; src: string | null } | null>(null);
  useEffect(() => {
    let active = true;
    if (!trackId || trackId < 1) return () => { active = false; };
    const load = () => void loadTrackArtwork(trackId).then((url) => { if (active) setLoadedArtwork({ trackId, src: url }); });
    if (eager || typeof IntersectionObserver === "undefined" || !hostRef.current) load();
    else {
      const observer = new IntersectionObserver((entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          observer.disconnect();
          load();
        }
      }, { rootMargin: "240px" });
      observer.observe(hostRef.current);
      return () => { active = false; observer.disconnect(); };
    }
    return () => { active = false; };
  }, [eager, trackId]);

  const src = loadedArtwork && loadedArtwork.trackId === trackId ? loadedArtwork.src : null;
  if (src) return <div ref={hostRef} className={`artwork artwork-${size} artwork-loaded`} role="img" aria-label={`Cover art for ${label}`}><img src={src} alt="" decoding="async" /></div>;
  return (
    <div ref={hostRef} className={`artwork artwork-${size}`} style={{ "--art-hue": 218 } as CSSProperties} role="img" aria-label={`MyPod placeholder artwork for ${label}`}>
      <span className="art-orbit art-orbit-one" />
      <span className="art-orbit art-orbit-two" />
      <span className="art-core"><Icon name="music" size={size === "small" ? 16 : 26} /></span>
      <span className="art-caption">MyPod · artwork unavailable</span>
    </div>
  );
}

function TrackRow({ track, index, selected, playback, playbackEnabled, onSelect, onPlay, onAddToQueue }: { track: Track; index: number; selected: boolean; playback: PlaybackSnapshot; playbackEnabled: boolean; onSelect: (track: Track) => void; onPlay: (track: Track) => void; onAddToQueue: (track: Track) => void }) {
  const isPlaying = playback.track?.id === track.id && playback.phase === "playing";
  const isBusy = playback.track?.id === track.id && playback.phase === "loading";
  return (
    <div className={`track-row${selected ? " track-selected" : ""}${isPlaying ? " track-playing" : ""}`} role="listitem">
      <span className="track-index">{index + 1}</span>
      <Art label={track.album || track.title} size="small" trackId={track.id} />
      <div className="track-row-copy">
        <button className="track-title-select" type="button" aria-pressed={selected} onClick={() => onSelect(track)} aria-label={`Select ${track.title} for the player`}>
          {track.title}
        </button>
        <span className="track-subtitle">{track.artist || "Unknown artist"}</span>
      </div>
      <span className="track-album">{track.album || "Unknown album"}</span>
      <span className="track-time">{formatDuration(track.duration)}</span>
      <button className="queue-add" type="button" disabled={!playbackEnabled} onClick={() => onAddToQueue(track)} aria-label={playbackEnabled ? `Add ${track.title} to queue` : "Queue unavailable until iPhone playback is implemented"} title={playbackEnabled ? "Add to queue" : "Queue unavailable on iPhone yet"}><Icon name="plus" size={16} /></button>
      <button className="row-play" type="button" disabled={!playbackEnabled} onClick={() => onPlay(track)} aria-label={playbackEnabled ? `${isPlaying ? "Pause" : "Play"} ${track.title}` : "Playback unavailable on iPhone yet"} aria-busy={isBusy} title={isBusy ? "Loading audio" : undefined}>
        <Icon name={isPlaying ? "pause" : "play"} size={17} />
      </button>
    </div>
  );
}

function PlaybackSeekBar({ playback, playbackEnabled, label, onSeek, className = "ipod-progress" }: { playback: PlaybackSnapshot; playbackEnabled: boolean; label: string; onSeek: (seconds: number) => void; className?: string }) {
  const rangeRef = useRef<HTMLInputElement>(null);
  const pointerActive = useRef(false);
  const [draft, setDraft] = useState<number | null>(null);
  const duration = playback.duration && playback.duration > 0 ? playback.duration : null;
  const current = draft ?? Math.min(playback.currentTime, duration ?? 0);
  const canSeek = playbackEnabled && Boolean(playback.track) && duration !== null && playback.phase !== "error";
  const commit = () => {
    if (!pointerActive.current) return;
    pointerActive.current = false;
    const value = Number(rangeRef.current?.value);
    if (Number.isFinite(value)) onSeek(value);
    setDraft(null);
  };
  const interactKeys = new Set(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"]);

  useEffect(() => {
    setDraft(null);
    pointerActive.current = false;
  }, [playback.track?.id]);

  return <div className={className}>
    <span>{formatDuration(current)}</span>
    <input
      ref={rangeRef}
      type="range"
      min="0"
      max={duration ?? 1}
      step="0.1"
      value={current}
      disabled={!canSeek}
      aria-label={label}
      aria-valuetext={`${formatDuration(current)} of ${duration ? formatDuration(duration) : "unknown duration"}`}
      onPointerDown={(event) => {
        if (!canSeek) return;
        pointerActive.current = true;
        setDraft(Number(event.currentTarget.value));
        try { event.currentTarget.setPointerCapture(event.pointerId); } catch { /* WebView may manage capture itself. */ }
      }}
      onPointerUp={commit}
      onPointerCancel={commit}
      onLostPointerCapture={commit}
      onKeyDown={(event) => {
        if (interactKeys.has(event.key)) {
          pointerActive.current = true;
          setDraft(Number(event.currentTarget.value));
        }
      }}
      onKeyUp={(event) => { if (interactKeys.has(event.key)) commit(); }}
      onBlur={commit}
      onChange={(event) => {
        const value = Number(event.currentTarget.value);
        if (pointerActive.current) setDraft(value);
        else onSeek(value);
      }}
    />
    <span>{duration ? formatDuration(duration) : "--:--"}</span>
  </div>;
}

function CollectionCard({ title, subtitle, shape = "square", icon = "disc" as IconName, onClick, artworkTrackId }: { title: string; subtitle: string; shape?: "square" | "circle"; icon?: IconName; onClick?: () => void; artworkTrackId?: number }) {
  return (
    <button className="collection-card" onClick={onClick} type="button">
      <span className={`collection-art collection-art-${shape}`}>{artworkTrackId ? <Art label={title} size="card" trackId={artworkTrackId} /> : <><span className="collection-glow" /><Icon name={icon} size={30} /></>}</span>
      <span className="collection-title">{title}</span>
      <span className="collection-subtitle">{subtitle}</span>
    </button>
  );
}

/** Neutral cover shell; a future local artwork URL can be supplied without changing the gallery. */
function PlaylistCover({ artworkUrl }: { artworkUrl?: string | null }) {
  return <span className="playlist-cover" aria-hidden="true">
    {artworkUrl ? <img src={artworkUrl} alt="" loading="lazy" /> : <span className="playlist-cover-placeholder"><span className="playlist-cover-mark"><Icon name="music" size={32} /></span><span className="playlist-cover-lines" /></span>}
  </span>;
}

function handlePlaylistGalleryKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  const cards = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("[data-playlist-card]"));
  if (!cards.length) return;
  const focusedIndex = cards.indexOf(document.activeElement as HTMLButtonElement);
  const currentIndex = focusedIndex < 0 ? (event.key === "ArrowRight" ? -1 : 0) : focusedIndex;
  const nextIndex = Math.max(0, Math.min(cards.length - 1, currentIndex + (event.key === "ArrowRight" ? 1 : -1)));
  event.preventDefault();
  cards[nextIndex].focus();
  cards[nextIndex].scrollIntoView({
    block: "nearest",
    inline: "nearest",
    behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth",
  });
}

function IpodPlayer({ onClose, onCenterSelect, selectedTrack, playback, playbackEnabled }: { onClose: () => void; onCenterSelect: () => void; selectedTrack: Track | null; playback: PlaybackSnapshot; playbackEnabled: boolean }) {
  const closeButton = useRef<HTMLButtonElement>(null);
  const displayTrack = playback.track || selectedTrack;

  useEffect(() => {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    closeButton.current?.focus();
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== "Tab") return;
      const buttons = Array.from(document.querySelectorAll<HTMLButtonElement>(".ipod-stage button:not(:disabled)"));
      const first = buttons[0];
      const last = buttons[buttons.length - 1];
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => {
      window.removeEventListener("keydown", handleKey);
      previousFocus?.focus();
    };
  }, [onClose]);

  return (
    <div className="player-scrim" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <section className="ipod-stage" role="dialog" aria-modal="true" aria-labelledby="ipod-title" aria-describedby="ipod-description">
        <button ref={closeButton} className="player-close" onClick={onClose} aria-label="Close player" type="button"><Icon name="close" /></button>
        <div className="ipod-body">
          <div className="ipod-header"><span className="ipod-brand">MyPod</span><span className="ipod-mark" aria-hidden="true">∞</span></div>
          <div className="ipod-screen">
            <div className="ipod-screen-art"><Art label={displayTrack?.album || displayTrack?.title || "MyPod player"} size="large" trackId={displayTrack?.id} eager /></div>
            <div className="ipod-screen-meta">
              <span className="ipod-screen-label">{playback.track ? playback.phase : selectedTrack ? "Selected track" : "MyPod player"}</span>
              <strong id="ipod-title">{displayTrack?.title || "No track selected"}</strong>
              <span>{displayTrack ? `${displayTrack.artist || "Unknown artist"} · ${displayTrack.album || "Unknown album"}` : "Choose a track from your library to view its details here."}</span>
              <span className="ipod-playback-state" id="ipod-description" role={playback.error ? "alert" : "status"} aria-live="polite">{playbackEnabled ? (playback.error || (playback.track ? "Audio state is reported by the player." : selectedTrack ? "Selected only. Press play to start audio." : "Choose a track, then press play.")) : "iPhone import is available. Native audio playback is a later stage."}</span>
              <PlaybackSeekBar key={playback.track?.id ?? "no-track"} playback={playback} playbackEnabled={playbackEnabled} label="Seek playback" onSeek={(seconds) => playbackController.seek(seconds)} />
            </div>
          </div>
          <div className="click-wheel" role="group" aria-label="Player click wheel controls">
            <span className="wheel-ring" aria-hidden="true" />
            <button className="wheel-menu" onClick={onClose} type="button" aria-label="Menu: close player">MENU</button>
            <button className="wheel-action wheel-prev" type="button" disabled={!playbackEnabled || !playback.track} onClick={() => void playbackController.previous()} aria-label="Previous track"><Icon name="previous" size={20} /></button>
            <button className="wheel-action wheel-next" type="button" disabled={!playbackEnabled || playback.queue.length === 0} onClick={() => void playbackController.next()} aria-label={playback.queue.length ? "Play next queued track" : "Next track unavailable: queue is empty"}><Icon name="next" size={20} /></button>
            <button className="wheel-action wheel-play" type="button" disabled={!playbackEnabled || !displayTrack} onClick={() => { console.info("MyPod playback: iPod play control tapped"); playbackController.toggle(displayTrack); }} aria-label={playback.phase === "playing" ? "Pause playback" : "Play playback"} title={playback.phase === "playing" ? "Pause" : "Play"}><Icon name={playback.phase === "playing" ? "pause" : "play"} size={20} /></button>
            <button className="wheel-center" type="button" disabled={!displayTrack} onClick={onCenterSelect} aria-label="Select: find this track in your library">SELECT</button>
          </div>
        </div>
        <p className="ipod-caption">Menu closes the player. Select finds this track in your library. Next plays the first queued track.</p>
      </section>
    </div>
  );
}

function QueuePanel({ playback, onClose, onRemove, onClear }: { playback: PlaybackSnapshot; onClose: () => void; onRemove: (entryId: number) => void; onClear: () => void }) {
  return (
    <aside className="queue-panel" aria-labelledby="queue-title">
      <header className="queue-panel-heading"><div><p className="eyebrow">UP NEXT</p><h2 id="queue-title">Queue</h2></div><button type="button" className="queue-close" onClick={onClose} aria-label="Close queue"><Icon name="close" size={18} /></button></header>
      <section className="queue-current" aria-label="Current track">
        <span className="queue-label">{playback.phase === "playing" ? "NOW PLAYING" : playback.phase === "paused" ? "PAUSED TRACK" : playback.phase === "loading" ? "LOADING TRACK" : playback.phase === "error" ? "PLAYBACK ERROR" : playback.phase === "ended" ? "LAST TRACK" : "CURRENT TRACK"}</span>
        {playback.track ? <><strong>{playback.track.title}</strong><span>{playback.track.artist || "Unknown artist"} · {playback.phase}</span>{playback.error && <small role="status">{playback.error}</small>}</> : <span>No track is playing.</span>}
      </section>
      <div className="queue-upcoming-heading"><h3>Next in queue</h3><button type="button" onClick={onClear} disabled={playback.queue.length === 0}>Clear</button></div>
      {playback.queue.length ? <ol className="queue-list">{playback.queue.map((entry, index) => <li key={entry.entryId} className="queue-entry">
        <span className="queue-position">{index + 1}</span><div className="queue-entry-copy"><strong>{entry.track.title}</strong><span>{entry.track.artist || "Unknown artist"} · ID {entry.trackId}</span></div>
        <button type="button" onClick={() => onRemove(entry.entryId)} aria-label={`Remove ${entry.track.title} from queue`}><Icon name="close" size={15} /></button>
      </li>)}</ol> : <p className="queue-empty">Your queue is empty. Add a track from your library to play it next.</p>}
    </aside>
  );
}

function App() {
  const playback = useSyncExternalStore(playbackController.subscribe, playbackController.getSnapshot, playbackController.getSnapshot);
  const playbackEnabled = playbackAvailable;
  const [view, setView] = useState<View>("Home");
  const [tracks, setTracks] = useState<Track[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [libraryFilter, setLibraryFilter] = useState("Tracks");
  const [playerOpen, setPlayerOpen] = useState(false);
  const [queueOpen, setQueueOpen] = useState(false);
  const [selectedTrack, setSelectedTrack] = useState<Track | null>(null);
  const [activeAlbum, setActiveAlbum] = useState<Track | null>(null);
  const [importing, setImporting] = useState(false);
  const [importMessage, setImportMessage] = useState<string | null>(null);
  const [spotifyPreview, setSpotifyPreview] = useState<SpotifyCsvPreview | null>(null);
  const [matchReview, setMatchReview] = useState<AudioMatchReview | null>(null);
  const [matchTrackId, setMatchTrackId] = useState("");
  const [matchBusy, setMatchBusy] = useState(false);
  const [spotifyImportId, setSpotifyImportId] = useState<string | null>(null);
  const [spotifyImporting, setSpotifyImporting] = useState(false);
  const [spotifyPreviewPage, setSpotifyPreviewPage] = useState(0);
  const [previewingSpotifyCsv, setPreviewingSpotifyCsv] = useState(false);
  const [spotifyPreviewMessage, setSpotifyPreviewMessage] = useState<string | null>(null);
  const [backupBusy, setBackupBusy] = useState(false);
  const [backupMessage, setBackupMessage] = useState<string | null>(null);
  const [backupError, setBackupError] = useState<string | null>(null);
  const [pendingBackupExport, setPendingBackupExport] = useState<{ token: string; summary: BackupSummary } | null>(null);
  const [pendingBackupRestore, setPendingBackupRestore] = useState<{ token: string; summary: BackupSummary } | null>(null);
  const [iosDiagnostics, setIosDiagnostics] = useState<IosRuntimeDiagnostics | null>(null);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [activePlaylistId, setActivePlaylistId] = useState<number | null>(null);
  const [playlistEntries, setPlaylistEntries] = useState<PlaylistEntry[]>([]);
  const [playlistBusy, setPlaylistBusy] = useState(false);
  const [playlistError, setPlaylistError] = useState<string | null>(null);
  const [playlistDialog, setPlaylistDialog] = useState<"create" | "rename" | "add" | "delete" | null>(null);
  const [playlistName, setPlaylistName] = useState("");
  const playlistAdapter = libraryAdapter.playlists;
  const csvInput = useRef<HTMLInputElement>(null);
  const browserAudioInput = useRef<HTMLInputElement>(null);
  const browserAudioFolderInput = useRef<HTMLInputElement>(null);
  const iosRuntime = typeof navigator !== "undefined" && isIOSPlatform(navigator);
  const tauriRuntime = isTauri();
  const localAudioScanAvailable = Boolean(libraryAdapter.scanAudioFolder);
  const browserFolderPickerAvailable = typeof document !== "undefined" && "webkitdirectory" in document.createElement("input");

  const refreshPlaylists = async () => {
    if (!playlistAdapter) return;
    try {
      const result = await playlistAdapter.listPlaylists();
      setPlaylists(result);
      if (activePlaylistId !== null && !result.some((playlist) => playlist.id === activePlaylistId)) {
        setActivePlaylistId(null);
        setPlaylistEntries([]);
      }
    } catch (reason) {
      setPlaylistError(reason instanceof Error ? reason.message : "Playlists could not be loaded.");
    }
  };

  const selectPlaylist = async (playlistId: number) => {
    setActivePlaylistId(playlistId);
    setPlaylistError(null);
    if (!playlistAdapter) return;
    try {
      setPlaylistEntries(await playlistAdapter.listEntries(playlistId));
    } catch (reason) {
      setPlaylistError(reason instanceof Error ? reason.message : "Playlist tracks could not be loaded.");
    }
  };

  const loadLibrary = async (signal?: AbortSignal) => {
    setLoading(true);
    setError(null);
    try {
      const result = await libraryAdapter.listTracks(0, signal);
      setTracks(result.items);
      setTotal(result.total);
    } catch (reason) {
      if (reason instanceof DOMException && reason.name === "AbortError") return;
      setError(libraryAdapter.importAudio ? "The on-device catalog could not be loaded. Retry in a moment." : "The local library could not be loaded. Check the local API and try again.");
    } finally {
      if (!signal?.aborted) setLoading(false);
    }
  };

  const loadMore = async () => {
    setLoadingMore(true);
    try {
      const result = await libraryAdapter.listTracks(tracks.length);
      setTracks((current) => [...current, ...result.items]);
      setTotal(result.total);
    } catch {
      setError(libraryAdapter.importAudio ? "More on-device tracks could not be loaded. Retry in a moment." : "More tracks could not be loaded. Check the local API and try again.");
    } finally {
      setLoadingMore(false);
    }
  };

  useEffect(() => { const listener = (event: Event) => setIosDiagnostics((event as CustomEvent<IosRuntimeDiagnostics>).detail); window.addEventListener("mypod:ios-diagnostics", listener); return () => window.removeEventListener("mypod:ios-diagnostics", listener); }, []);

  useEffect(() => {
    const controller = new AbortController();
    void loadLibrary(controller.signal);
    return () => controller.abort();
  }, []);

  useEffect(() => {
    void refreshPlaylists();
  }, []);

  const submitPlaylistDialog = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!playlistAdapter) return;
    setPlaylistBusy(true);
    setPlaylistError(null);
    try {
      if (playlistDialog === "create") {
        const created = await playlistAdapter.createPlaylist(playlistName);
        setPlaylists((current) => [created, ...current]);
        setActivePlaylistId(created.id);
        setPlaylistEntries([]);
      } else if (playlistDialog === "rename" && activePlaylistId !== null) {
        const renamed = await playlistAdapter.renamePlaylist(activePlaylistId, playlistName);
        setPlaylists((current) => current.map((playlist) => playlist.id === renamed.id ? renamed : playlist));
      } else if (playlistDialog === "delete" && activePlaylistId !== null) {
        await playlistAdapter.deletePlaylist(activePlaylistId);
        setPlaylists((current) => current.filter((playlist) => playlist.id !== activePlaylistId));
        setActivePlaylistId(null);
        setPlaylistEntries([]);
      }
      setPlaylistDialog(null);
      setPlaylistName("");
      await refreshPlaylists();
    } catch (reason) {
      setPlaylistError(reason instanceof Error ? reason.message : "The playlist action could not be completed.");
    } finally {
      setPlaylistBusy(false);
    }
  };

  const addTrackToPlaylist = async (trackId: number) => {
    if (!playlistAdapter || activePlaylistId === null) return;
    setPlaylistBusy(true);
    setPlaylistError(null);
    try {
      const entry = await playlistAdapter.addTrack(activePlaylistId, trackId);
      setPlaylistEntries((current) => [...current, entry]);
      await refreshPlaylists();
    } catch (reason) {
      setPlaylistError(reason instanceof Error ? reason.message : "Track could not be added.");
    } finally {
      setPlaylistBusy(false);
    }
  };

  const removePlaylistEntry = async (entryId: number) => {
    if (!playlistAdapter || activePlaylistId === null) return;
    try {
      await playlistAdapter.removeEntry(activePlaylistId, entryId);
      await selectPlaylist(activePlaylistId);
      await refreshPlaylists();
    } catch (reason) {
      setPlaylistError(reason instanceof Error ? reason.message : "Playlist entry could not be removed.");
    }
  };

  const movePlaylistEntry = async (entryIndex: number, direction: -1 | 1) => {
    if (!playlistAdapter || activePlaylistId === null) return;
    const target = entryIndex + direction;
    if (target < 0 || target >= playlistEntries.length) return;
    const order = playlistEntries.map((entry) => entry.id);
    [order[entryIndex], order[target]] = [order[target], order[entryIndex]];
    try {
      setPlaylistEntries(await playlistAdapter.reorderEntries(activePlaylistId, order));
      await refreshPlaylists();
    } catch (reason) {
      setPlaylistError(reason instanceof Error ? reason.message : "Playlist order could not be saved.");
    }
  };

  const playPlaylistEntry = async (entry: PlaylistEntry) => {
    if (entry.track_id === null) return;
    try {
      const track = tracks.find((candidate) => candidate.id === entry.track_id) ?? await libraryAdapter.getTrack(entry.track_id);
      setSelectedTrack(track);
      await playbackController.play(track);
    } catch (reason) {
      setPlaylistError(reason instanceof Error ? reason.message : "This playlist track could not be played.");
    }
  };

  const playTrack = (track: Track) => {
    console.info("MyPod playback: track-row play control tapped");
    void playbackController.play(track);
  };

  const artists = useMemo(() => [...new Set(tracks.map((track) => track.artist).filter((artist): artist is string => Boolean(artist)))].sort((a, b) => a.localeCompare(b)), [tracks]);
  const albums = useMemo(() => [...new Map(tracks.filter((track) => track.album).map((track) => [`${track.album}\u0000${track.artist || ""}`, track])).values()], [tracks]);
  const visibleTracks = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    const filtered = tracks.filter((track) => !normalized || [track.title, track.artist, track.album].some((value) => value?.toLocaleLowerCase().includes(normalized)));
    if (view === "Search" || view === "Your Library") return filtered;
    return filtered.slice(0, 8);
  }, [query, tracks, view]);

  const navigate = (nextView: View) => {
    setView(nextView);
    if (nextView !== "Search") setQuery("");
  };

  const searchFor = (term: string) => {
    setQuery(term);
    setView("Search");
  };

  const openAlbum = (track: Track) => {
    setActiveAlbum(track);
    setQuery("");
    setView("Album");
  };

  const albumTracks = useMemo(() => activeAlbum?.album
    ? tracks.filter((track) => track.album?.localeCompare(activeAlbum.album || "", undefined, { sensitivity: "accent" }) === 0 && (track.artist || "") === (activeAlbum.artist || ""))
    : [], [activeAlbum, tracks]);

  const importAudio = async () => {
    setImporting(true);
    setImportMessage(null);
    try {
      if (libraryAdapter.scanAudioFolder) {
        const result = await libraryAdapter.scanAudioFolder();
        if (result.status === "cancelled") return;
        const counts = result.result;
        setImportMessage(`Folder scan finished: ${counts.imported} added, ${counts.updated} updated, ${counts.skipped} unchanged, ${counts.failed} errors, ${counts.missing} missing. Source files were indexed in place.`);
      } else if (libraryAdapter.importAudio) {
        const result = await libraryAdapter.importAudio();
        if (result.status === "cancelled") return;
        setImportMessage((result.status === "duplicate" ? `Already imported: ${result.track.title}` : `Imported: ${result.track.title}`) + (result.metadataWarning ? ` · ${result.metadataWarning}` : ""));
      } else {
        browserAudioInput.current?.click();
        return;
      }
      await loadLibrary();
      if (activePlaylistId !== null && playlistAdapter) setPlaylistEntries(await playlistAdapter.listEntries(activePlaylistId));
    } catch (reason) {
      setImportMessage(reason instanceof Error ? reason.message : "The audio file could not be imported.");
    } finally {
      setImporting(false);
    }
  };

  const chooseBrowserAudioFolder = () => browserAudioFolderInput.current?.click();

  const handleBrowserAudioSelection = (event: ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(event.currentTarget.files ?? []);
    event.currentTarget.value = "";
    if (!files.length) return;
    const mp3Files = files.filter((file) => file.name.toLocaleLowerCase().endsWith(".mp3") || file.type === "audio/mpeg");
    setImportMessage(mp3Files.length
      ? `Selected ${mp3Files.length} MP3 file${mp3Files.length === 1 ? "" : "s"}. Browser access is permission-based; this local API has no browser upload endpoint, so these files were not copied or indexed. Use MyPod desktop's folder scan to index files in place.`
      : "No MP3 files were selected. Nothing was uploaded, copied, or indexed.");
  };

  const previewSpotifyCsv = async (file?: File) => {
    if (!libraryAdapter.previewSpotifyCsv) return;
    if (!iosRuntime && !file) {
      csvInput.current?.click();
      return;
    }
    setPreviewingSpotifyCsv(true);
    setSpotifyPreviewMessage(null);
    setSpotifyPreview(null);
    setSpotifyImportId(null);
    setSpotifyPreviewPage(0);
    try {
      const result = await libraryAdapter.previewSpotifyCsv(file);
      if (result.status === "cancelled") return;
      setSpotifyPreview(result.preview);
      setSpotifyImportId(result.importId ?? null);
      setSpotifyPreviewPage(0);
      console.info("[MyPod CSV] UI state updated", { complete: result.preview.complete, sourceRecords: result.preview.summary.source_records, rows: result.preview.rows.length });
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      console.error("[MyPod CSV] preview flow failed", { error: message });
      setSpotifyPreviewMessage(message.includes("CSV file access failed") || message.includes("security") || message.includes("permission") || message.includes("access")
        ? "MyPod could not read that file. Re-select it and try again."
        : message.includes("CSV") || message.includes("header") || message.includes("parse")
          ? `CSV validation failed: ${message}`
          : iosRuntime
            ? "MyPod could not preview the selected file. Re-select it from Files and try again."
            : "MyPod could not preview that CSV. Check the local API and try again.");
    } finally {
      setPreviewingSpotifyCsv(false);
    }
  };

  const reviewMatch = async (catalogId: number) => {
    if (!libraryAdapter.listAudioMatchReview) return;
    setMatchBusy(true);
    try { const [review] = await libraryAdapter.listAudioMatchReview(catalogId); setMatchReview(review ?? null); setMatchTrackId(review?.track_id?.toString() ?? ""); }
    catch (reason) { setPlaylistError(reason instanceof Error ? reason.message : "Review could not be loaded."); }
    finally { setMatchBusy(false); }
  };
  const saveMatch = async (trackId: number | null) => {
    if (!matchReview || !libraryAdapter.setAudioMatch) return;
    setMatchBusy(true);
    try {
      await libraryAdapter.setAudioMatch(matchReview.catalog_id, trackId);
      if (activePlaylistId !== null && playlistAdapter) setPlaylistEntries(await playlistAdapter.listEntries(activePlaylistId));
      setMatchReview(null);
    } catch (reason) { setPlaylistError(reason instanceof Error ? reason.message : "Match could not be saved."); }
    finally { setMatchBusy(false); }
  };

  const confirmSpotifyImport = async () => {
    if (!spotifyImportId || !libraryAdapter.confirmSpotifyImport) return;
    setSpotifyImporting(true);
    try {
      const result = await libraryAdapter.confirmSpotifyImport(spotifyImportId);
      setSpotifyPreviewMessage(result.already_imported ? "This source was already imported." : `Imported ${result.playlists} playlists and ${result.entries} entries. ${result.preserved_other_rows} other source rows preserved separately.`);
      setSpotifyImportId(null);
      await refreshPlaylists();
    } catch (reason) { setSpotifyPreviewMessage(reason instanceof Error ? reason.message : "Import failed. No partial import was saved."); }
    finally { setSpotifyImporting(false); }
  };

  const createLocalBackup = async (scope: BackupScope) => {
    const adapter = libraryAdapter.backups;
    if (!adapter) return;
    setBackupBusy(true);
    setBackupError(null);
    setBackupMessage(null);
    try {
      const result = await adapter.createBackup(scope);
      if (result.status === "cancelled") return;
      if (result.exportError) {
        setPendingBackupExport({ token: result.token, summary: result.summary });
        setBackupError(`The backup bundle was created and retained on this device, but export failed: ${result.exportError}`);
      } else {
        setBackupMessage(`${scope === "complete" ? "Complete database-and-media" : "Database-only"} backup saved. ${result.summary.file_count} files · schema ${result.summary.schema_version} · ${result.summary.sha256.slice(0, 12)}…`);
        setPendingBackupExport(null);
      }
    } catch (reason) {
      setBackupError(reason instanceof Error ? reason.message : "Backup could not be created.");
    } finally {
      setBackupBusy(false);
    }
  };

  const retryBackupExport = async () => {
    if (!libraryAdapter.backups || !pendingBackupExport) return;
    setBackupBusy(true);
    setBackupError(null);
    try {
      const exported = await libraryAdapter.backups.exportBackup(pendingBackupExport.token);
      if (exported) {
        setBackupMessage(`Backup exported. SHA-256 ${pendingBackupExport.summary.sha256}`);
        setPendingBackupExport(null);
      } else {
        setBackupMessage("Export cancelled. The verified backup remains on this device and can be exported again.");
      }
    } catch (reason) {
      setBackupError(reason instanceof Error ? reason.message : "Backup export failed.");
    } finally {
      setBackupBusy(false);
    }
  };

  const inspectLocalBackup = async () => {
    if (!libraryAdapter.backups) return;
    setBackupBusy(true);
    setBackupError(null);
    setBackupMessage(null);
    setPendingBackupRestore(null);
    try {
      const result = await libraryAdapter.backups.inspectBackup();
      if (result.status === "cancelled") return;
      setPendingBackupRestore({ token: result.token, summary: result.summary });
      setBackupMessage(`Backup verified: ${result.summary.scope === "complete" ? "database and media" : "database only"} · schema ${result.summary.schema_version} · ${result.summary.file_count} files · SHA-256 ${result.summary.sha256.slice(0, 12)}…`);
    } catch (reason) {
      setBackupError(reason instanceof Error ? reason.message : "Backup validation failed.");
    } finally {
      setBackupBusy(false);
    }
  };

  const restoreLocalBackup = async () => {
    if (!libraryAdapter.backups || !pendingBackupRestore) return;
    setBackupBusy(true);
    setBackupError(null);
    try {
      const summary = await libraryAdapter.backups.restoreBackup(pendingBackupRestore.token);
      setBackupMessage(`Restore completed and database integrity passed. Schema ${summary.schema_version}; ${summary.file_count} verified bundle files.`);
      setPendingBackupRestore(null);
      await loadLibrary();
      await refreshPlaylists();
    } catch (reason) {
      setBackupError(reason instanceof Error ? reason.message : "Restore failed. The existing catalog remains available unless the atomic database replacement completed.");
    } finally {
      setBackupBusy(false);
    }
  };
  const spotifyGroups = spotifyPreview?.rows.reduce<Array<{ label: string; count: number }>>((groups, row, index, rows) => {
    if (row.mapped.source_type !== "Playlist" || !row.mapped.playlist_name?.trim()) return groups;
    const previous = rows[index - 1]?.mapped;
    if (previous?.source_type === "Playlist" && previous.playlist_name === row.mapped.playlist_name) groups[groups.length - 1].count++;
    else groups.push({ label: row.mapped.playlist_name, count: 1 });
    return groups;
  }, []) ?? [];

  const previewRows = spotifyPreview?.rows.slice(
    spotifyPreviewPage * SPOTIFY_PREVIEW_PAGE_SIZE,
    (spotifyPreviewPage + 1) * SPOTIFY_PREVIEW_PAGE_SIZE,
  ) ?? [];

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <a className="brand" href="#home" onClick={(event) => { event.preventDefault(); navigate("Home"); }} aria-label="MyPod Home">
          <img className="brand-logo" src={myPodLogo} alt="" /><span>MyPod</span>
        </a>
        <nav className="primary-nav" aria-label="Primary navigation">
          <button className={`nav-item ${view === "Home" ? "nav-active" : ""}`} aria-current={view === "Home" ? "page" : undefined} onClick={() => navigate("Home")}><Icon name="home" /><span>Home</span></button>
          <button className={`nav-item ${view === "Search" ? "nav-active" : ""}`} aria-current={view === "Search" ? "page" : undefined} onClick={() => navigate("Search")}><Icon name="search" /><span>Search</span></button>
          <button className={`nav-item ${view === "Your Library" ? "nav-active" : ""}`} aria-current={view === "Your Library" ? "page" : undefined} onClick={() => navigate("Your Library")}><Icon name="library" /><span>Your Library</span></button>
        </nav>
        <div className="sidebar-rule" />
        <section className="sidebar-library" aria-label="Your library">
          <div className="sidebar-library-heading"><h2>Your Library</h2><button type="button" aria-label={playlistAdapter ? "Create playlist" : "Create playlist unavailable"} title={playlistAdapter ? "Create playlist" : "Playlist creation is not available yet"} disabled={!playlistAdapter} onClick={() => { setPlaylistName(""); setPlaylistDialog("create"); }}><Icon name="plus" size={20} /></button></div>
          <div className="sidebar-library-filters" aria-label="Library categories">
            {(["Tracks", "Albums", "Artists", "Playlists"] as const).map((filter) => <button key={filter} type="button" onClick={() => { setLibraryFilter(filter); navigate("Your Library"); }}>{filter}</button>)}
          </div>
          <div className="sidebar-list-tools"><button type="button" aria-label="Search library" onClick={() => searchFor("")}><Icon name="search" size={19} /></button><span>Recently added</span></div>
          <div className="sidebar-collection-list">
            <button className={`sidebar-collection-item ${view === "Liked Songs" ? "sidebar-item-active" : ""}`} type="button" onClick={() => navigate("Liked Songs")}>
              <span className="sidebar-liked-art"><Icon name="heart" size={20} /></span><span className="sidebar-item-copy"><strong>Liked Songs</strong><small>Saved tracks unavailable</small></span>
            </button>
            {albums.slice(0, 6).map((album) => <button className="sidebar-collection-item" key={`${album.album}-${album.artist}`} type="button" onClick={() => openAlbum(album)}>
              <Art label={album.album || album.title} size="small" trackId={album.id} /><span className="sidebar-item-copy"><strong>{album.album || "Unknown album"}</strong><small>Album · {album.artist || "Unknown artist"}</small></span>
            </button>)}
            {artists.slice(0, 4).map((artist) => <button className="sidebar-collection-item" key={`artist-${artist}`} type="button" onClick={() => searchFor(artist)}>
              <span className="sidebar-artist-art"><Icon name="artist" size={20} /></span><span className="sidebar-item-copy"><strong>{artist}</strong><small>Artist</small></span>
            </button>)}
            {!loading && !error && albums.length === 0 && artists.length === 0 && <p className="sidebar-no-library">Your scanned albums and artists will appear here.</p>}
          </div>
          {playlistAdapter ? playlists.map((playlist) => <button key={playlist.id} className={`sidebar-collection-item ${activePlaylistId === playlist.id ? "sidebar-item-active" : ""}`} type="button" onClick={() => { setLibraryFilter("Playlists"); navigate("Your Library"); void selectPlaylist(playlist.id); }}><span className="sidebar-artist-art"><Icon name="library" size={18} /></span><span className="sidebar-item-copy"><strong>{playlist.name}</strong><small>Playlist · {playlist.track_count} {playlist.track_count === 1 ? "track" : "tracks"}</small></span></button>) : <div className="playlist-empty"><span className="playlist-empty-dot" /><p>No playlists yet</p><small>Playlist tools are coming later.</small></div>}
        </section>
        <div className="sidebar-bottom"><span className="privacy-dot" /> On this device</div>
      </aside>

      <main className="main-panel">
        <div className="main-scroll">
          <header className="topbar">
            <span className="mobile-brand"><img src={myPodLogo} alt="" />MyPod</span>
            <div className="topbar-start">
              <div className="history-controls"><button type="button" aria-label="Back" disabled><Icon name="arrow-left" /></button><button type="button" aria-label="Forward" disabled><Icon name="arrow-right" /></button></div>
              <button className="home-shortcut" type="button" onClick={() => navigate("Home")} aria-label="Home"><Icon name="home" size={21} /></button>
              <label className="desktop-search"><Icon name="search" size={21} /><input value={query} onFocus={() => setView("Search")} onChange={(event) => { setQuery(event.target.value); setView("Search"); }} placeholder="Search your library" aria-label="Search your library" /></label>
            </div>
            <div className="topbar-right">
              <button className="mobile-search-shortcut" type="button" onClick={() => navigate("Search")} aria-label="Search your library"><Icon name="search" size={20} /></button>
              <span className="local-badge"><span /> On this device</span><button className="profile-button" type="button" aria-label="Local profile">M</button>
            </div>
          </header>

          {view === "Search" && <div className="search-field mobile-search-field"><Icon name="search" /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search tracks, artists, albums" aria-label="Search your library" autoFocus /></div>}

          <div className="page-content">
            {view === "Album" && activeAlbum && <>
              <section className="album-detail-heading" aria-labelledby="album-detail-title">
                <button type="button" className="album-detail-back" onClick={() => { setLibraryFilter("Albums"); setView("Your Library"); }}>Your Library · Albums</button>
                <Art label={activeAlbum.album || activeAlbum.title} size="large" trackId={activeAlbum.id} eager />
                <div><p className="eyebrow">ALBUM</p><h1 id="album-detail-title">{activeAlbum.album || "Unknown album"}</h1><p>{activeAlbum.artist || "Unknown artist"} · {albumTracks.length}{tracks.length < total ? "+" : ""} tracks currently loaded</p></div>
              </section>
              <TrackSection title="Tracks" tracks={albumTracks} selectedTrackId={selectedTrack?.id} playback={playback} playbackEnabled={playbackEnabled} onSelectTrack={setSelectedTrack} onPlay={playTrack} onAddToQueue={(track) => playbackController.enqueue(track)} emptyCopy="No tracks from this album are loaded yet. Load more library tracks to find them." />
              <LoadMore count={tracks.length} total={total} loading={loadingMore} onLoad={loadMore} />
            </>}
            {view === "Home" && <>
              <header className="home-heading">
                <div><p className="eyebrow">ON THIS DEVICE</p><h1>Your music</h1></div>
                <button className="home-library-link" type="button" onClick={() => navigate("Your Library")}>Browse library <Icon name="arrow-right" size={16} /></button>
              </header>
              {albums.length > 0 && <section className="quick-row" aria-label="Album shortcuts">
                {albums.slice(0, 4).map((album) => <button key={`${album.album}-${album.artist}`} onClick={() => openAlbum(album)} aria-label={`Open album ${album.album || "unknown album"}`}><Art label={album.album || album.title} size="small" trackId={album.id} /><span className="quick-label">{album.album || "Unknown album"}</span></button>)}
              </section>}
              <ApiStatus loading={loading} error={error} retry={() => void loadLibrary()} />
              <section className="content-section">
                <SectionTitle title="Albums in your library" action="Your Library" onAction={() => { setLibraryFilter("Albums"); navigate("Your Library"); }} />
              {tracks.length === 0 ? <EmptyState icon="music" title={error ? "Local library is unavailable" : loading ? "Checking your library" : "Your library is ready for its first scan"} copy={error ? (libraryAdapter.importAudio ? "Retry the on-device catalog. No imported tracks are available until it responds." : "Check the local API and retry; no track data is available until it responds.") : loading ? "Waiting for your local library." : libraryAdapter.importAudio ? "Import an audio file from Files to add it to this iPhone." : "Once you scan a permitted music folder, your albums and artists will show up here."} /> : (
                  <div className="collection-grid collection-shelf">
                    {albums.slice(0, 4).map((album) => <CollectionCard key={`${album.album}-${album.artist}`} title={album.album || "Unknown album"} subtitle={`Album · ${album.artist || "Unknown artist"}`} icon="disc" />)}
                    {albums.length === 0 && <EmptyState icon="disc" title="No album tags yet" copy="Tracks without album metadata are still available in your library." />}
                  </div>
                )}
              </section>
              {tracks.length > 0 && artists.length > 0 && <section className="content-section">
                <SectionTitle title="Artists in your library" action="Browse artists" onAction={() => { setLibraryFilter("Artists"); navigate("Your Library"); }} />
                <div className="collection-grid collection-shelf artist-shelf">
                  {artists.slice(0, 8).map((artist) => <CollectionCard key={artist} title={artist} subtitle="Artist from your library" shape="circle" icon="artist" />)}
                </div>
              </section>}
              {tracks.length > 0 && <TrackSection title="In your library" tracks={visibleTracks} selectedTrackId={selectedTrack?.id} playback={playback} playbackEnabled={playbackEnabled} onSelectTrack={setSelectedTrack} onPlay={playTrack} onAddToQueue={(track) => playbackController.enqueue(track)} />}
            </>}

            {view === "Search" && <>
              <div className="page-heading"><p className="eyebrow">FIND SOMETHING FAMILIAR</p><h1>Search</h1><p>Search tracks, artists, and albums among tracks currently loaded from your library.</p></div>
              <ApiStatus loading={loading} error={error} retry={() => void loadLibrary()} />
              {query.trim() ? <><TrackSection title={`Results in loaded tracks for “${query.trim()}”`} tracks={visibleTracks} selectedTrackId={selectedTrack?.id} playback={playback} playbackEnabled={playbackEnabled} onSelectTrack={setSelectedTrack} onPlay={playTrack} onAddToQueue={(track) => playbackController.enqueue(track)} emptyCopy="No matching tracks in the loaded part of your library." /><LoadMore count={tracks.length} total={total} loading={loadingMore} onLoad={loadMore} /></> : <EmptyState icon="search" title="Search your library" copy="Type a track, artist, or album name to find it on this device." />}
            </>}

              {matchReview && <section className="spotify-preview" aria-label="Review audio match">
                <h2>Audio match: {matchReview.title}</h2><p>{matchReview.artist} · {matchReview.album}</p>
                <p>{matchReview.track_id ? `${matchReview.method} link · ${matchReview.available ? "Audio available" : "Audio missing"}` : "No verified audio link"}. Confirm only the same recording and version. Your choice applies to every playlist containing this catalog track.</p>
                {matchReview.candidates.map((c) => <p key={c.track_id}>{c.title} · {c.artist} · {c.album}: {c.reason}{!c.available && " (file missing)"}</p>)}
                <label>Local audio <select value={matchTrackId} onChange={(e) => setMatchTrackId(e.target.value)}><option value="">Choose a file</option>{[...new Map([...matchReview.candidates.map((c) => ({id:c.track_id,title:c.title,is_missing:c.available ? 0 : 1})),...tracks].map((t) => [t.id,t])).values()].map((t) => <option key={t.id} value={t.id} disabled={Boolean(t.is_missing)}>{t.title}{t.is_missing ? " (missing)" : ""}</option>)}</select></label>
                <button type="button" disabled={!matchTrackId || matchBusy} onClick={() => void saveMatch(Number(matchTrackId))}>Confirm audio match</button>
                <button type="button" disabled={matchBusy} onClick={() => void saveMatch(null)}>Unlink audio</button>
                <button type="button" disabled={matchBusy} onClick={() => setMatchReview(null)}>Cancel</button>
                <LoadMore count={tracks.length} total={total} loading={loadingMore} onLoad={loadMore} />
              </section>}

            {view === "Your Library" && <>
              <div className="page-heading library-heading"><div><p className="eyebrow">YOUR COLLECTION</p><h1>Your Library</h1><p>{total ? `${total} tracks indexed on this device` : "Your local collection, all in one place."}</p></div><div className="library-actions">
                {!iosRuntime && libraryAdapter.previewSpotifyCsv && <input className="file-picker-hidden" ref={csvInput} type="file" accept=".csv,text/csv" aria-label="Choose Spotify CSV file" onChange={(event) => { const file = event.currentTarget.files?.[0]; event.currentTarget.value = ""; if (file) void previewSpotifyCsv(file); }} />}
                {!iosRuntime && !tauriRuntime && <>
                  <input className="file-picker-hidden" ref={browserAudioInput} type="file" accept=".mp3,audio/mpeg" multiple aria-label="Choose MP3 audio files" onChange={handleBrowserAudioSelection} />
                  <input className="file-picker-hidden" ref={(element) => { browserAudioFolderInput.current = element; element?.setAttribute("webkitdirectory", ""); }} type="file" accept=".mp3,audio/mpeg" multiple aria-label="Choose a folder of MP3 audio files" onChange={handleBrowserAudioSelection} />
                </>}
                {libraryAdapter.previewSpotifyCsv && <button className="import-audio-button" type="button" onClick={() => void previewSpotifyCsv()} disabled={previewingSpotifyCsv}>{previewingSpotifyCsv ? "Reading CSV…" : iosRuntime ? "Preview Spotify CSV" : "Import Spotify CSV"}</button>}
                {(libraryAdapter.importAudio || localAudioScanAvailable || !iosRuntime) && <button className="import-audio-button" type="button" onClick={() => void importAudio()} disabled={importing}>{importing ? "Working…" : "Import Audio"}</button>}
                {!iosRuntime && !tauriRuntime && browserFolderPickerAvailable && <button className="import-audio-button secondary-import-action" type="button" onClick={chooseBrowserAudioFolder}>Choose folder</button>}
                <button className="round-action" type="button" onClick={() => void loadLibrary()} aria-label="Refresh library"><Icon name="refresh" /></button>
              </div></div>
              <div className="library-import-notes" aria-label="Import information">
                {!iosRuntime && <p>Spotify CSV is preview-only on web and desktop; this backend does not save CSV imports.</p>}
                {localAudioScanAvailable && <p>Folder scans index files in place. The selected directory must be inside a configured permitted scan root.</p>}
                {!iosRuntime && !tauriRuntime && <p>Browser file and folder selection grants temporary access only. Selected MP3 files are not uploaded, copied, or indexed; use the MyPod desktop folder scanner to add them in place.</p>}
              </div>
              {libraryAdapter.backups && <section className="backup-panel" aria-labelledby="backup-panel-title">
                <div><p className="eyebrow">ON-DEVICE RECOVERY</p><h2 id="backup-panel-title">Backup &amp; restore</h2><p>Created on this device and saved to a Files location you choose. Choose an on-device location for local-only storage; Files providers may sync selected files.</p></div>
                <div className="backup-actions">
                  <button type="button" onClick={() => void createLocalBackup("complete")} disabled={backupBusy}>{backupBusy ? "Working…" : "Back up database + media"}</button>
                  <button type="button" onClick={() => void createLocalBackup("database_only")} disabled={backupBusy}>{backupBusy ? "Working…" : "Database-only backup"}</button>
                  <button type="button" onClick={() => void inspectLocalBackup()} disabled={backupBusy}>{backupBusy ? "Working…" : "Choose backup to restore"}</button>
                  {pendingBackupExport && <button type="button" onClick={() => void retryBackupExport()} disabled={backupBusy}>Retry exporting retained backup</button>}
                </div>
                {backupMessage && <p className="backup-message" role="status" aria-live="polite">{backupMessage}</p>}
                {backupError && <p className="backup-error" role="alert">{backupError}</p>}
                {pendingBackupRestore && <div className="backup-review" role="region" aria-label="Validated backup details">
                  <strong>Backup integrity verified</strong>
                  <span>{pendingBackupRestore.summary.scope === "complete" ? "Database and managed media" : "Database only"} · schema {pendingBackupRestore.summary.schema_version} · {pendingBackupRestore.summary.file_count} files · SHA-256 {pendingBackupRestore.summary.sha256.slice(0, 16)}…</span>
                  <p>Restoring replaces the catalog and playlists after validation. Existing managed audio is never deleted; identical files are reused and new bundle media is merged. A database-only restore requires matching local media to already exist.</p>
                  <button type="button" className="backup-restore-button" disabled={backupBusy} onClick={() => void restoreLocalBackup()}>I understand — replace catalog and restore</button>
                </div>}
              </section>}
              {importMessage && <p className="import-status" role="status" aria-live="polite">{importMessage}</p>}
              {import.meta.env.VITE_MYPOD_IOS_DIAGNOSTICS === "true" && libraryAdapter.previewSpotifyCsv && <details className="spotify-preview"><summary>iOS runtime diagnostics</summary><button type="button" onClick={() => void invoke<IosRuntimeDiagnostics>("ios_runtime_diagnostics", { selectedPath: null, stagingSucceeded: false, previewSucceeded: false }).then(setIosDiagnostics).catch((error) => setIosDiagnostics({ bridge_works: false, native_build_id: "unknown", database_schema_version: null, migration_status: "unknown", csv_selection_returned_path: false, selected_file_exists: null, staging_succeeded: false, preview_succeeded: false, failure_stage: "bridge", sanitized_error: String(error) }))}>Check native bridge</button>{iosDiagnostics && <dl><dt>Bridge</dt><dd>{iosDiagnostics.bridge_works ? "Connected" : "Failed"}</dd><dt>Native build</dt><dd>{iosDiagnostics.native_build_id}</dd><dt>Database</dt><dd>Schema {iosDiagnostics.database_schema_version ?? "unavailable"} · {iosDiagnostics.migration_status}</dd><dt>CSV selection</dt><dd>{iosDiagnostics.csv_selection_returned_path ? "Path received" : "No path observed"} · {iosDiagnostics.selected_file_exists === null ? "not checked" : iosDiagnostics.selected_file_exists ? "original file visible" : "original provider URL unavailable"} · staging {iosDiagnostics.staging_succeeded ? "succeeded" : "not reported"} · parsing {iosDiagnostics.preview_succeeded ? "succeeded" : "not reported"}</dd><dt>Failure stage</dt><dd>{iosDiagnostics.failure_stage}{iosDiagnostics.sanitized_error ? ` · ${iosDiagnostics.sanitized_error}` : ""}</dd></dl>}</details>}
              {libraryAdapter.previewSpotifyCsv && <section className="spotify-preview" aria-labelledby="spotify-preview-title">
                <div className="spotify-preview-intro"><div><h2 id="spotify-preview-title">Spotify CSV preview</h2><p>{libraryAdapter.confirmSpotifyImport ? "Review first, then confirm to save metadata and playlists on this device. Audio can be added later." : "Preview only: this desktop/web backend does not persist Spotify CSV imports. Previewing never writes to the library."}</p></div>{spotifyPreview && <button type="button" className="preview-dismiss" onClick={() => setSpotifyPreview(null)}>Close preview</button>}</div>
                {spotifyPreviewMessage && <p className="spotify-preview-message" role="alert">{spotifyPreviewMessage}</p>}
                {spotifyPreview && <>
                  <div className={`spotify-preview-summary${spotifyPreview.complete ? "" : " preview-incomplete"}`} role="status" aria-live="polite">
                    <strong>{spotifyPreview.complete ? "Preview complete" : "Partial preview — later records could not be recovered"}</strong>
                    <span>{spotifyPreview.summary.preview_rows} non-empty records shown · {spotifyPreview.summary.ignored_empty_records} empty records ignored · {spotifyPreview.summary.rows_with_errors} rows with errors · {spotifyPreview.summary.rows_with_warnings} warnings</span>
                  </div>
                  <p>Source row order is provisional; it may differ from Spotify order. Each contiguous block of an exact Playlist name becomes a separate imported playlist. Same-named playlists within one block cannot be distinguished because this CSV has no playlist IDs. Favorites, Album, Artist and unnamed rows are preserved separately.</p>
                  <p>{spotifyGroups.length} playlist blocks · {spotifyPreview.rows.filter((r) => r.mapped.source_type === "Playlist" && !r.mapped.playlist_name?.trim()).length} playlist rows missing a name</p>
                  <details><summary>Playlist groups</summary><ul>{spotifyGroups.map((g, i) => <li key={i}>{g.label}: {g.count} entries</li>)}</ul></details>
                  {spotifyImportId && <button type="button" disabled={spotifyImporting || !spotifyPreview.complete || spotifyPreview.file_errors.length > 0 || spotifyPreview.summary.rows_with_errors > 0} onClick={() => void confirmSpotifyImport()}>{spotifyImporting ? "Importing…" : "Confirm metadata and playlist import"}</button>}
                  {spotifyPreview.file_errors.length > 0 && <ul className="spotify-preview-issues" aria-label="File-level diagnostics">{spotifyPreview.file_errors.map((issue, index) => <li key={`${issue.code}-${index}`}><strong>{issue.code.replaceAll("_", " ")}</strong>{issue.message && ` — ${issue.message}`}{issue.columns && `: ${issue.columns.join(", ")}`}{issue.physical_line_start && ` (physical lines ${issue.physical_line_start}–${issue.physical_line_end})`}</li>)}</ul>}
                  {spotifyPreview.summary.ignored_empty_spans.length > 0 && <details className="spotify-preview-duplicates"><summary>Empty source rows and line spans</summary><ul>{spotifyPreview.summary.ignored_empty_spans.map((span) => <li key={`${span.source_record_start}-${span.source_record_end}`}>Source record{span.source_record_start === span.source_record_end ? "" : "s"} {span.source_record_start}{span.source_record_end === span.source_record_start ? "" : `–${span.source_record_end}`} · physical lines {span.physical_line_start}–{span.physical_line_end}</li>)}</ul></details>}
                  {spotifyPreview.duplicate_reports.length > 0 && <details className="spotify-preview-duplicates"><summary>{spotifyPreview.duplicate_reports.length} duplicate or metadata conflict reports</summary><ul>{spotifyPreview.duplicate_reports.map((report) => <li key={report.report_id}><strong>{report.category.replaceAll("_", " ")}</strong> · source records {report.source_records.join(", ")}{report.differing_fields?.length ? ` · differing: ${report.differing_fields.join(", ")}` : ""}</li>)}</ul></details>}
                  {spotifyPreview.summary.preview_rows > 0 && <>
                    <p className="spotify-preview-page-note">Showing source records {previewRows[0]?.source_record}–{previewRows.at(-1)?.source_record} of {spotifyPreview.summary.preview_rows}. IDs and Type values are opaque source values.</p>
                    <div className="spotify-preview-table-wrap" role="region" aria-label="CSV records; scroll horizontally to see all columns" tabIndex={0}><table className="spotify-preview-table"><thead><tr><th scope="col">Record / lines</th><th scope="col">Track</th><th scope="col">Artist</th><th scope="col">Album</th><th scope="col">Playlist label</th><th scope="col">Type (source)</th><th scope="col">Spotify ID (opaque)</th><th scope="col">Review</th><th scope="col">Original source cells</th></tr></thead><tbody>{previewRows.map((row) => <tr key={row.source_record}><td>{row.source_record}<small>{row.physical_line_start === row.physical_line_end ? `line ${row.physical_line_start}` : `lines ${row.physical_line_start}–${row.physical_line_end}`}</small></td><td>{row.mapped.track_name || "—"}</td><td>{row.mapped.artist_name || "—"}</td><td>{row.mapped.album || "—"}</td><td>{row.mapped.playlist_name || "—"}</td><td>{row.mapped.source_type || "—"}</td><td>{row.mapped.spotify_id || "—"}</td><td>{[...row.errors, ...row.warnings].map((issue) => issue.code.replaceAll("_", " ")).concat(row.duplicate_reports.map((id) => `report ${id}`)).join("; ") || "Reviewable"}</td><td><details><summary>View cells</summary><pre>{JSON.stringify(row.source_values)}</pre></details></td></tr>)}</tbody></table></div>
                    <nav className="spotify-preview-pagination" aria-label="Preview pages"><button type="button" onClick={() => setSpotifyPreviewPage((page) => Math.max(0, page - 1))} disabled={spotifyPreviewPage === 0}>Previous records</button><span>Page {spotifyPreviewPage + 1} of {Math.ceil(spotifyPreview.summary.preview_rows / SPOTIFY_PREVIEW_PAGE_SIZE)}</span><button type="button" onClick={() => setSpotifyPreviewPage((page) => Math.min(Math.ceil(spotifyPreview.summary.preview_rows / SPOTIFY_PREVIEW_PAGE_SIZE) - 1, page + 1))} disabled={(spotifyPreviewPage + 1) * SPOTIFY_PREVIEW_PAGE_SIZE >= spotifyPreview.summary.preview_rows}>Next records</button></nav>
                  </>}
                </>}
              </section>}
              <div className="filter-pills" role="tablist" aria-label="Library filters">{["Tracks", "Artists", "Albums", "Playlists"].map((filter) => <button role="tab" aria-selected={libraryFilter === filter} className={libraryFilter === filter ? "pill-active" : ""} key={filter} onClick={() => setLibraryFilter(filter)}>{filter}</button>)}</div>
              <ApiStatus loading={loading} error={error} retry={() => void loadLibrary()} />
              {libraryFilter === "Tracks" && <TrackSection title="Tracks" tracks={visibleTracks} selectedTrackId={selectedTrack?.id} playback={playback} playbackEnabled={playbackEnabled} onSelectTrack={setSelectedTrack} onPlay={playTrack} onAddToQueue={(track) => playbackController.enqueue(track)} emptyCopy={libraryAdapter.importAudio ? "Import an audio file from Files to add it to your library." : "Your scanned tracks will appear here."} />}
              {libraryFilter === "Artists" && <CollectionSection title="Artists" items={artists.map((artist) => ({ title: artist, subtitle: "Artist", shape: "circle" as const, icon: "artist" as IconName }))} emptyTitle="No artists yet" />}
              {libraryFilter === "Albums" && <CollectionSection title="Albums" items={albums.map((album) => ({ title: album.album || "Unknown album", subtitle: album.artist || "Unknown artist", icon: "disc" as IconName, artworkTrackId: album.id, onClick: () => openAlbum(album) }))} emptyTitle="No albums yet" />}
              {libraryFilter === "Playlists" && (playlistAdapter ? <section className="playlist-manager" aria-label="Playlists">
                {playlistError && <p className="playlist-error" role="alert">{playlistError}</p>}
                {activePlaylistId === null ? <>
                  <div className="section-title"><h2>Your playlists</h2><button type="button" onClick={() => { setPlaylistName(""); setPlaylistDialog("create"); }}><Icon name="plus" size={16} /> New playlist</button></div>
                  {playlists.length ? <><p className="visually-hidden" id="playlist-gallery-help">Swipe or scroll horizontally to browse. Use the left and right arrow keys to move between playlists.</p><div className="playlist-gallery" role="list" aria-label="Your playlists" aria-describedby="playlist-gallery-help" tabIndex={0} onKeyDown={handlePlaylistGalleryKeyDown}>{[...playlists].sort((a, b) => Number(Boolean(a.source_playlist_identifier)) - Number(Boolean(b.source_playlist_identifier))).map((playlist) => <div className="playlist-card-item" role="listitem" key={playlist.id}><button className="playlist-card" type="button" data-playlist-card onClick={() => void selectPlaylist(playlist.id)} aria-label={`Open ${playlist.name}, ${playlist.track_count} ${playlist.track_count === 1 ? "track" : "tracks"}`}><PlaylistCover /><strong>{playlist.name}</strong><small>{playlist.source_playlist_identifier?.startsWith("spotify-import:") ? "Imported Spotify · " : "My playlist · "}{playlist.track_count} {playlist.track_count === 1 ? "track" : "tracks"}</small></button></div>)}</div></> : <EmptyState icon="library" title="No playlists yet" copy="Create a playlist, then add tracks from this on-device library." />}
                </> : <>
                  <div className="playlist-detail-heading"><button type="button" className="playlist-back" onClick={() => setActivePlaylistId(null)}>All playlists</button><div className="playlist-heading-copy"><p className="eyebrow">ON THIS DEVICE</p><h2>{playlists.find((playlist) => playlist.id === activePlaylistId)?.name || "Playlist"}</h2><p>{playlistEntries.length} {playlistEntries.length === 1 ? "entry" : "entries"} · {playlists.find((p) => p.id === activePlaylistId)?.source_playlist_identifier?.startsWith("spotify-import:") ? "provisional source order; edits saved locally" : "display order saved on this device"}</p></div><div className="playlist-detail-actions"><button type="button" onClick={() => { setPlaylistName(playlists.find((playlist) => playlist.id === activePlaylistId)?.name || ""); setPlaylistDialog("rename"); }}>Rename</button><button type="button" onClick={() => setPlaylistDialog("delete")}>Delete</button><button type="button" onClick={() => setPlaylistDialog("add")} disabled={tracks.length === 0}>Add tracks</button></div></div>
                  {playlistEntries.length ? <ol className="playlist-entry-list">{playlistEntries.map((entry, index) => <li key={entry.id} className="playlist-entry-row"><span className="playlist-entry-position">{index + 1}</span><Art label={entry.album || entry.title} size="small" trackId={entry.track_id} /><div className="playlist-entry-copy"><strong>{entry.title}</strong><span>{entry.artist || (entry.track_id === null ? "Unmatched source track · kept for review" : "Unknown artist")}{entry.album ? ` · ${entry.album}` : ""}{entry.catalog_id ? (entry.track_id === null ? " · Missing audio" : " · Audio linked") : ""}</span>{entry.catalog_id && libraryAdapter.setAudioMatch && <button type="button" disabled={matchBusy} onClick={() => void reviewMatch(entry.catalog_id!)}>Review audio</button>}</div>{entry.track_id !== null ? <button type="button" className="playlist-entry-play" onClick={() => void playPlaylistEntry(entry)} disabled={!playbackEnabled} aria-label={`Play ${entry.title}`}><Icon name="play" size={16} /></button> : <span className="playlist-unmatched">Missing audio</span>}<div className="playlist-order-controls"><button type="button" onClick={() => void movePlaylistEntry(index, -1)} disabled={index === 0 || playlistBusy} aria-label={`Move ${entry.title} up`}>↑</button><button type="button" onClick={() => void movePlaylistEntry(index, 1)} disabled={index === playlistEntries.length - 1 || playlistBusy} aria-label={`Move ${entry.title} down`}>↓</button></div><button type="button" className="playlist-entry-remove" onClick={() => void removePlaylistEntry(entry.id)} aria-label={`Remove ${entry.title} from playlist`}><Icon name="close" size={16} /></button></li>)}</ol> : <EmptyState icon="music" title="This playlist is empty" copy="Add one or more tracks from your on-device library. Repeated entries are allowed." />}
                </>}
              </section> : <EmptyState icon="library" title="Playlist support is not available here" copy="Playlists are currently stored in the iPhone's on-device catalog." />)}
              <LoadMore count={tracks.length} total={total} loading={loadingMore} onLoad={loadMore} />
            </>}

            {view === "Liked Songs" && <>
              <section className="liked-hero"><div className="liked-large"><Icon name="heart" size={38} /></div><div><p className="eyebrow">YOUR COLLECTION</p><h1>Liked Songs</h1><p>Keep the songs you love close.</p></div></section>
              <EmptyState icon="heart" title="Saved tracks are not available yet" copy="There is no favorites collection in the library API yet, so nothing is shown as liked." />
            </>}
          </div>
        </div>

        <footer className="playback-bar">
          <div className="now-track"><button className="now-track-open" type="button" disabled={!playback.track && !selectedTrack} onClick={() => setPlayerOpen(true)} aria-label="Open iPod Now Playing player"><span className="mini-art"><Art label={(playback.track || selectedTrack)?.album || (playback.track || selectedTrack)?.title || "MyPod"} size="small" trackId={(playback.track || selectedTrack)?.id} eager /></span><span className="now-copy"><strong>{playback.track?.title || selectedTrack?.title || "Nothing selected"}</strong><span role={playback.error ? "alert" : "status"} aria-live="polite">{playback.track ? playback.error || playback.phase : selectedTrack ? `${selectedTrack.artist || "Unknown artist"} · selected, not playing` : "Choose a track to begin"}</span></span></button><button className="heart-quiet" type="button" aria-label="Favorites unavailable" disabled><Icon name="heart" size={17} /></button></div>
          <div className="transport"><div className="transport-buttons"><button disabled aria-label="Shuffle unavailable" title="Shuffle is not implemented"><Icon name="shuffle" size={17} /></button><button disabled={!playbackEnabled || !playback.track} onClick={() => void playbackController.previous()} aria-label="Previous track" title="Restart current track after 3 seconds, otherwise return to history"><Icon name="previous" size={19} /></button><button className="play-main" disabled={!playbackEnabled || (!playback.track && !selectedTrack)} onClick={() => { console.info("MyPod playback: bottom-bar play control tapped"); playbackController.toggle(selectedTrack); }} aria-label={playbackEnabled ? (playback.phase === "playing" ? "Pause" : playback.phase === "loading" ? "Cancel loading" : "Play") : "Playback unavailable on iPhone yet"} title={playback.error || undefined}><Icon name={playback.phase === "playing" ? "pause" : "play"} size={17} /></button><button disabled={!playbackEnabled || playback.queue.length === 0} onClick={() => void playbackController.next()} aria-label={playback.queue.length ? "Play next queued track" : "Next track unavailable: queue is empty"} title="Play the first queued track"><Icon name="next" size={19} /></button><button disabled aria-label="Repeat unavailable" title="Repeat is not implemented"><Icon name="repeat" size={17} /></button></div><PlaybackSeekBar className="seek-row" playback={playback} playbackEnabled={playbackEnabled} label="Playback position" onSeek={(seconds) => playbackController.seek(seconds)} /></div>
          <div className="playback-tools"><button className="queue-toggle" type="button" aria-label={`Open queue, ${playback.queue.length} upcoming`} aria-expanded={queueOpen} onClick={() => setQueueOpen((open) => !open)}><Icon name="queue" size={17} /><span>{playback.queue.length}</span></button><button className="open-player" onClick={() => setPlayerOpen(true)} type="button"><span className="device-glyph">◉</span><span>Now Playing</span></button><Icon name="volume" size={18} /><input type="range" min="0" max="100" value="0" readOnly disabled aria-label="Volume unavailable" /></div>
        </footer>
      </main>

      <nav className="mobile-nav" aria-label="Mobile navigation">
        <button className={view === "Home" ? "mobile-nav-active" : ""} aria-current={view === "Home" ? "page" : undefined} onClick={() => navigate("Home")}><Icon name="home" size={20} /><span>Home</span></button>
        <button className={view === "Search" ? "mobile-nav-active" : ""} aria-current={view === "Search" ? "page" : undefined} onClick={() => navigate("Search")}><Icon name="search" size={20} /><span>Search</span></button>
        <button className={view === "Your Library" ? "mobile-nav-active" : ""} aria-current={view === "Your Library" ? "page" : undefined} onClick={() => navigate("Your Library")}><Icon name="library" size={20} /><span>Library</span></button>
        <button aria-haspopup="dialog" onClick={() => setPlayerOpen(true)}><Icon name="disc" size={20} /><span>Player</span></button>
      </nav>
      {queueOpen && <QueuePanel playback={playback} onClose={() => setQueueOpen(false)} onRemove={(entryId) => playbackController.removeFromQueue(entryId)} onClear={() => playbackController.clearQueue()} />}
      {playlistDialog && <div className="playlist-dialog-scrim" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) setPlaylistDialog(null); }}><section className="playlist-dialog" role="dialog" aria-modal="true" aria-labelledby="playlist-dialog-title">
        <header><h2 id="playlist-dialog-title">{playlistDialog === "create" ? "Create playlist" : playlistDialog === "rename" ? "Rename playlist" : playlistDialog === "delete" ? "Delete playlist?" : "Add tracks"}</h2><button type="button" onClick={() => setPlaylistDialog(null)} aria-label="Close dialog"><Icon name="close" size={18} /></button></header>
        {playlistDialog === "add" ? <div className="playlist-picker-list">{tracks.length ? tracks.map((track) => <div className="playlist-picker-row" key={track.id}><div><strong>{track.title}</strong><span>{track.artist || "Unknown artist"}</span></div><button type="button" onClick={() => void addTrackToPlaylist(track.id)} disabled={playlistBusy}>Add</button></div>) : <EmptyState icon="music" title="No loaded tracks" copy="Import audio or load tracks before adding them." />}</div> : <form onSubmit={(event) => void submitPlaylistDialog(event)}>
          {playlistDialog === "delete" ? <p>Playlist entries will be removed. Imported audio files will stay in your library.</p> : <label>Playlist name<input autoFocus value={playlistName} onChange={(event) => setPlaylistName(event.target.value)} maxLength={120} required /></label>}
          {playlistError && <p className="playlist-error" role="alert">{playlistError}</p>}
          <footer><button type="button" onClick={() => setPlaylistDialog(null)}>Cancel</button><button type="submit" disabled={playlistBusy}>{playlistBusy ? "Saving…" : playlistDialog === "delete" ? "Delete playlist" : "Save"}</button></footer>
        </form>}
      </section></div>}
      {playerOpen && <IpodPlayer onClose={() => setPlayerOpen(false)} onCenterSelect={() => {
        const track = playback.track || selectedTrack;
        if (!track) return;
        setSelectedTrack(track);
        setQuery(track.title);
        setView("Search");
        setPlayerOpen(false);
      }} selectedTrack={selectedTrack} playback={playback} playbackEnabled={playbackEnabled} />}
    </div>
  );
}

function ApiStatus({ loading, error, retry }: { loading: boolean; error: string | null; retry: () => void }) {
  if (loading) return <div className="api-note" role="status" aria-live="polite"><span className="loading-dot" /> Checking your local library…</div>;
  if (error) return <div className="api-error" role="status" aria-live="polite"><span>{error}</span><button onClick={retry} type="button"><Icon name="refresh" size={16} /> Retry</button></div>;
  return null;
}

function SectionTitle({ title, action, onAction }: { title: string; action?: string; onAction?: () => void }) {
  return <div className="section-title"><h2>{title}</h2>{action && <button onClick={onAction} type="button">{action}<Icon name="arrow-right" size={16} /></button>}</div>;
}

function TrackSection({ title, tracks, selectedTrackId, playback, playbackEnabled, onSelectTrack, onPlay, onAddToQueue, emptyCopy = "No matching tracks were found." }: { title: string; tracks: Track[]; selectedTrackId?: number; playback: PlaybackSnapshot; playbackEnabled: boolean; onSelectTrack: (track: Track) => void; onPlay: (track: Track) => void; onAddToQueue: (track: Track) => void; emptyCopy?: string }) {
  return <section className="content-section track-section"><SectionTitle title={title} />{tracks.length ? <div className="track-list" role="list"><div className="track-header"><span>#</span><span>Track</span><span>Album</span><span>Time</span><span aria-hidden="true" /><span aria-hidden="true" /></div>{tracks.map((track, index) => <TrackRow key={track.id} track={track} index={index} selected={track.id === selectedTrackId} playback={playback} playbackEnabled={playbackEnabled} onSelect={onSelectTrack} onPlay={onPlay} onAddToQueue={onAddToQueue} />)}</div> : <EmptyState icon="music" title="No tracks to show" copy={emptyCopy} />}</section>;
}

function CollectionSection({ title, items, emptyTitle }: { title: string; items: { title: string; subtitle: string; shape?: "square" | "circle"; icon: IconName; artworkTrackId?: number; onClick?: () => void }[]; emptyTitle: string }) {
  return <section className="content-section"><SectionTitle title={title} />{items.length ? <div className="collection-grid">{items.map((item) => <CollectionCard key={item.title} {...item} />)}</div> : <EmptyState icon={title === "Artists" ? "artist" : "disc"} title={emptyTitle} copy="A local scan will add real library metadata here." />}</section>;
}

function EmptyState({ icon, title, copy }: { icon: IconName; title: string; copy: string }) {
  return <div className="empty-state"><span className="empty-icon"><Icon name={icon} size={23} /></span><div><strong>{title}</strong><p>{copy}</p></div></div>;
}

function LoadMore({ count, total, loading, onLoad }: { count: number; total: number; loading: boolean; onLoad: () => void }) {
  if (count >= total || total === 0) return null;
  return <div className="load-more"><span>Showing {count} of {total} tracks</span><button type="button" onClick={onLoad} disabled={loading}>{loading ? "Loading…" : "Load more"}</button></div>;
}

export default App;
