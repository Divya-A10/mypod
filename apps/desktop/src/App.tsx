import { useEffect, useMemo, useRef, useState, useSyncExternalStore, type CSSProperties, type ReactNode } from "react";
import { formatDuration, getTracks, type Track } from "./api";
import { playbackController, type PlaybackSnapshot } from "./playback/PlaybackController";

type View = "Home" | "Search" | "Your Library" | "Liked Songs";
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

function artworkHue(seed: string): number {
  let hash = 0;
  for (const character of seed) hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
  return hash % 360;
}

function Art({ label, size = "card" }: { label: string; size?: "card" | "small" | "large" }) {
  const hue = artworkHue(label || "MyPod");
  return (
    <div className={`artwork artwork-${size}`} style={{ "--art-hue": hue } as CSSProperties} aria-hidden="true">
      <span className="art-orbit art-orbit-one" />
      <span className="art-orbit art-orbit-two" />
      <span className="art-core"><Icon name="music" size={size === "small" ? 16 : 26} /></span>
      <span className="art-caption">MyPod · artwork unavailable</span>
    </div>
  );
}

function TrackRow({ track, index, selected, playback, onSelect, onPlay, onAddToQueue }: { track: Track; index: number; selected: boolean; playback: PlaybackSnapshot; onSelect: (track: Track) => void; onPlay: (track: Track) => void; onAddToQueue: (track: Track) => void }) {
  const isPlaying = playback.track?.id === track.id && playback.phase === "playing";
  const isBusy = playback.track?.id === track.id && playback.phase === "loading";
  return (
    <div className={`track-row${selected ? " track-selected" : ""}${isPlaying ? " track-playing" : ""}`} role="listitem">
      <span className="track-index">{index + 1}</span>
      <Art label={track.album || track.title} size="small" />
      <div className="track-row-copy">
        <button className="track-title-select" type="button" aria-pressed={selected} onClick={() => onSelect(track)} aria-label={`Select ${track.title} for the player`}>
          {track.title}
        </button>
        <span className="track-subtitle">{track.artist || "Unknown artist"}</span>
      </div>
      <span className="track-album">{track.album || "Unknown album"}</span>
      <span className="track-time">{formatDuration(track.duration)}</span>
      <button className="queue-add" type="button" onClick={() => onAddToQueue(track)} aria-label={`Add ${track.title} to queue`} title="Add to queue"><Icon name="plus" size={16} /></button>
      <button className="row-play" type="button" onClick={() => onPlay(track)} aria-label={`${isPlaying ? "Pause" : "Play"} ${track.title}`} aria-busy={isBusy} title={isBusy ? "Loading audio" : undefined}>
        <Icon name={isPlaying ? "pause" : "play"} size={17} />
      </button>
    </div>
  );
}

function CollectionCard({ title, subtitle, shape = "square", icon = "disc" as IconName, onClick }: { title: string; subtitle: string; shape?: "square" | "circle"; icon?: IconName; onClick?: () => void }) {
  return (
    <button className="collection-card" onClick={onClick} type="button">
      <span className={`collection-art collection-art-${shape}`}><span className="collection-glow" /><Icon name={icon} size={30} /></span>
      <span className="collection-title">{title}</span>
      <span className="collection-subtitle">{subtitle}</span>
    </button>
  );
}

function IpodPlayer({ onClose, selectedTrack, playback }: { onClose: () => void; selectedTrack: Track | null; playback: PlaybackSnapshot }) {
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
            <div className="ipod-screen-art"><Art label={displayTrack?.album || displayTrack?.title || "MyPod player"} size="large" /></div>
            <div className="ipod-screen-meta">
              <span className="ipod-screen-label">{playback.track ? playback.phase : selectedTrack ? "Selected track" : "MyPod player"}</span>
              <strong id="ipod-title">{displayTrack?.title || "No track selected"}</strong>
              <span>{displayTrack ? `${displayTrack.artist || "Unknown artist"} · ${displayTrack.album || "Unknown album"}` : "Choose a track from your library to view its details here."}</span>
              <span className="ipod-playback-state" id="ipod-description">{playback.error || (playback.track ? "Audio state is reported by the player." : selectedTrack ? "Selected only. Press play to start audio." : "Choose a track, then press play.")}</span>
              <div className="ipod-progress"><span>{formatDuration(playback.currentTime)}</span><input type="range" min="0" max={playback.duration || 1} value={Math.min(playback.currentTime, playback.duration || 0)} disabled={!playback.track || !playback.duration || playback.phase === "error"} onChange={(event) => playbackController.seek(Number(event.target.value))} aria-label="Seek playback"/><span>{formatDuration(playback.duration)}</span></div>
            </div>
          </div>
          <div className="click-wheel" role="group" aria-label="Player click wheel controls">
            <span className="wheel-ring" aria-hidden="true" />
            <button className="wheel-menu" onClick={onClose} type="button" aria-label="Menu: close player">MENU</button>
            <button className="wheel-action wheel-prev" type="button" disabled={!playback.track} onClick={() => void playbackController.previous()} aria-label="Previous track"><Icon name="previous" size={20} /><span>PREV</span></button>
            <button className="wheel-action wheel-next" type="button" disabled={playback.queue.length === 0} onClick={() => void playbackController.next()} aria-label={playback.queue.length ? "Play next queued track" : "Next track unavailable: queue is empty"}><Icon name="next" size={20} /><span>NEXT</span></button>
            <button className="wheel-action wheel-play" type="button" disabled={!displayTrack} onClick={() => playbackController.toggle(displayTrack)} aria-label={playback.phase === "playing" ? "Pause playback" : "Play playback"}><Icon name={playback.phase === "playing" ? "pause" : "play"} size={19} /><span>{playback.phase === "playing" ? "PAUSE" : "PLAY"}</span></button>
            <button className="wheel-center" type="button" disabled title="There is no player menu or selection action yet" aria-label="Center select unavailable">SELECT</button>
          </div>
        </div>
        <p className="ipod-caption">Menu closes the player. Next plays the first queued track; previous restarts or returns to history.</p>
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

  const loadLibrary = async (signal?: AbortSignal) => {
    setLoading(true);
    setError(null);
    try {
      const result = await getTracks(0, signal);
      setTracks(result.items);
      setTotal(result.total);
    } catch (reason) {
      if (reason instanceof DOMException && reason.name === "AbortError") return;
      setError("The local library could not be loaded. Check the local API and try again.");
    } finally {
      if (!signal?.aborted) setLoading(false);
    }
  };

  const loadMore = async () => {
    setLoadingMore(true);
    try {
      const result = await getTracks(tracks.length);
      setTracks((current) => [...current, ...result.items]);
      setTotal(result.total);
    } catch {
      setError("More tracks could not be loaded. Check the local API and try again.");
    } finally {
      setLoadingMore(false);
    }
  };

  useEffect(() => {
    const controller = new AbortController();
    void loadLibrary(controller.signal);
    return () => controller.abort();
  }, []);

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

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <a className="brand" href="#home" onClick={(event) => { event.preventDefault(); navigate("Home"); }} aria-label="MyPod Home">
          <span className="brand-mark"><span /><span /><span /></span><span>MyPod</span>
        </a>
        <nav className="primary-nav" aria-label="Primary navigation">
          <button className={`nav-item ${view === "Home" ? "nav-active" : ""}`} aria-current={view === "Home" ? "page" : undefined} onClick={() => navigate("Home")}><Icon name="home" /><span>Home</span></button>
          <button className={`nav-item ${view === "Search" ? "nav-active" : ""}`} aria-current={view === "Search" ? "page" : undefined} onClick={() => navigate("Search")}><Icon name="search" /><span>Search</span></button>
          <button className={`nav-item ${view === "Your Library" ? "nav-active" : ""}`} aria-current={view === "Your Library" ? "page" : undefined} onClick={() => navigate("Your Library")}><Icon name="library" /><span>Your Library</span></button>
        </nav>
        <div className="sidebar-rule" />
        <section className="sidebar-library" aria-label="Your library">
          <div className="sidebar-library-heading"><h2>Your Library</h2><button type="button" aria-label="Create playlist unavailable" title="Playlist creation is not available yet" disabled><Icon name="plus" size={20} /></button></div>
          <div className="sidebar-library-filters" aria-label="Library categories">
            {(["Tracks", "Albums", "Artists", "Playlists"] as const).map((filter) => <button key={filter} type="button" onClick={() => { setLibraryFilter(filter); navigate("Your Library"); }}>{filter}</button>)}
          </div>
          <div className="sidebar-list-tools"><button type="button" aria-label="Search library" onClick={() => searchFor("")}><Icon name="search" size={19} /></button><span>Recently added</span></div>
          <div className="sidebar-collection-list">
            <button className={`sidebar-collection-item ${view === "Liked Songs" ? "sidebar-item-active" : ""}`} type="button" onClick={() => navigate("Liked Songs")}>
              <span className="sidebar-liked-art"><Icon name="heart" size={20} /></span><span className="sidebar-item-copy"><strong>Liked Songs</strong><small>Saved tracks unavailable</small></span>
            </button>
            {albums.slice(0, 6).map((album) => <button className="sidebar-collection-item" key={`${album.album}-${album.artist}`} type="button" onClick={() => searchFor(album.album || "")}>
              <Art label={album.album || album.title} size="small" /><span className="sidebar-item-copy"><strong>{album.album || "Unknown album"}</strong><small>Album · {album.artist || "Unknown artist"}</small></span>
            </button>)}
            {artists.slice(0, 4).map((artist) => <button className="sidebar-collection-item" key={`artist-${artist}`} type="button" onClick={() => searchFor(artist)}>
              <span className="sidebar-artist-art"><Icon name="artist" size={20} /></span><span className="sidebar-item-copy"><strong>{artist}</strong><small>Artist</small></span>
            </button>)}
            {!loading && !error && albums.length === 0 && artists.length === 0 && <p className="sidebar-no-library">Your scanned albums and artists will appear here.</p>}
          </div>
          <div className="playlist-empty"><span className="playlist-empty-dot" /><p>No playlists yet</p><small>Playlist tools are coming later.</small></div>
        </section>
        <div className="sidebar-bottom"><span className="privacy-dot" /> On this device</div>
      </aside>

      <main className="main-panel">
        <div className="main-scroll">
          <header className="topbar">
            <span className="mobile-brand">MyPod</span>
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
            {view === "Home" && <>
              <header className="home-heading">
                <div><p className="eyebrow">ON THIS DEVICE</p><h1>Your music</h1></div>
                <button className="home-library-link" type="button" onClick={() => navigate("Your Library")}>Browse library <Icon name="arrow-right" size={16} /></button>
              </header>
              {albums.length > 0 && <section className="quick-row" aria-label="Album shortcuts">
                {albums.slice(0, 4).map((album) => <button key={`${album.album}-${album.artist}`} onClick={() => searchFor(album.album || "")} aria-label={`Search tracks from ${album.album || "unknown album"}`}><Art label={album.album || album.title} size="small" /><span className="quick-label">{album.album || "Unknown album"}</span></button>)}
              </section>}
              <ApiStatus loading={loading} error={error} retry={() => void loadLibrary()} />
              <section className="content-section">
                <SectionTitle title="Albums in your library" action="Your Library" onAction={() => { setLibraryFilter("Albums"); navigate("Your Library"); }} />
                {tracks.length === 0 ? <EmptyState icon="music" title={error ? "Local library is unavailable" : loading ? "Checking your library" : "Your library is ready for its first scan"} copy={error ? "Check the local API and retry; no track data is available until it responds." : loading ? "Waiting for the local library API." : "Once you scan a permitted music folder, your albums and artists will show up here."} /> : (
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
                {tracks.length > 0 && <TrackSection title="In your library" tracks={visibleTracks} selectedTrackId={selectedTrack?.id} playback={playback} onSelectTrack={setSelectedTrack} onPlay={(track) => void playbackController.play(track)} onAddToQueue={(track) => playbackController.enqueue(track)} />}
            </>}

            {view === "Search" && <>
              <div className="page-heading"><p className="eyebrow">FIND SOMETHING FAMILIAR</p><h1>Search</h1><p>Search tracks, artists, and albums among tracks currently loaded from your library.</p></div>
              <ApiStatus loading={loading} error={error} retry={() => void loadLibrary()} />
              {query.trim() ? <><TrackSection title={`Results in loaded tracks for “${query.trim()}”`} tracks={visibleTracks} selectedTrackId={selectedTrack?.id} playback={playback} onSelectTrack={setSelectedTrack} onPlay={(track) => void playbackController.play(track)} onAddToQueue={(track) => playbackController.enqueue(track)} emptyCopy="No matching tracks in the loaded part of your library." /><LoadMore count={tracks.length} total={total} loading={loadingMore} onLoad={loadMore} /></> : <EmptyState icon="search" title="Search your library" copy="Type a track, artist, or album name to find it on this device." />}
            </>}

            {view === "Your Library" && <>
              <div className="page-heading library-heading"><div><p className="eyebrow">YOUR COLLECTION</p><h1>Your Library</h1><p>{total ? `${total} tracks indexed on this device` : "Your local collection, all in one place."}</p></div><button className="round-action" type="button" onClick={() => void loadLibrary()} aria-label="Refresh library"><Icon name="refresh" /></button></div>
              <div className="filter-pills" role="tablist" aria-label="Library filters">{["Tracks", "Artists", "Albums", "Playlists"].map((filter) => <button role="tab" aria-selected={libraryFilter === filter} className={libraryFilter === filter ? "pill-active" : ""} key={filter} onClick={() => setLibraryFilter(filter)}>{filter}</button>)}</div>
              <ApiStatus loading={loading} error={error} retry={() => void loadLibrary()} />
              {libraryFilter === "Tracks" && <TrackSection title="Tracks" tracks={visibleTracks} selectedTrackId={selectedTrack?.id} playback={playback} onSelectTrack={setSelectedTrack} onPlay={(track) => void playbackController.play(track)} onAddToQueue={(track) => playbackController.enqueue(track)} emptyCopy="Your scanned tracks will appear here." />}
              {libraryFilter === "Artists" && <CollectionSection title="Artists" items={artists.map((artist) => ({ title: artist, subtitle: "Artist", shape: "circle" as const, icon: "artist" as IconName }))} emptyTitle="No artists yet" />}
              {libraryFilter === "Albums" && <CollectionSection title="Albums" items={albums.map((album) => ({ title: album.album || "Unknown album", subtitle: album.artist || "Unknown artist", icon: "disc" as IconName }))} emptyTitle="No albums yet" />}
              {libraryFilter === "Playlists" && <EmptyState icon="library" title="Playlist support is coming later" copy="MyPod does not have playlist data or playlist APIs yet." />}
              <LoadMore count={tracks.length} total={total} loading={loadingMore} onLoad={loadMore} />
            </>}

            {view === "Liked Songs" && <>
              <section className="liked-hero"><div className="liked-large"><Icon name="heart" size={38} /></div><div><p className="eyebrow">YOUR COLLECTION</p><h1>Liked Songs</h1><p>Keep the songs you love close.</p></div></section>
              <EmptyState icon="heart" title="Saved tracks are not available yet" copy="There is no favorites collection in the library API yet, so nothing is shown as liked." />
            </>}
          </div>
        </div>

        <footer className="playback-bar">
          <div className="now-track"><div className="mini-art"><Icon name="music" size={17} /></div><div className="now-copy"><strong>{playback.track?.title || selectedTrack?.title || "Nothing selected"}</strong><span>{playback.track ? playback.error || playback.phase : selectedTrack ? `${selectedTrack.artist || "Unknown artist"} · selected, not playing` : "Choose a track to begin"}</span></div><button className="heart-quiet" type="button" aria-label="Favorites unavailable" disabled><Icon name="heart" size={17} /></button></div>
          <div className="transport"><div className="transport-buttons"><button disabled aria-label="Shuffle unavailable" title="Shuffle is not implemented"><Icon name="shuffle" size={17} /></button><button disabled={!playback.track} onClick={() => void playbackController.previous()} aria-label="Previous track" title="Restart current track after 3 seconds, otherwise return to history"><Icon name="previous" size={19} /></button><button className="play-main" disabled={!playback.track && !selectedTrack} onClick={() => playbackController.toggle(selectedTrack)} aria-label={playback.phase === "playing" ? "Pause" : playback.phase === "loading" ? "Cancel loading" : "Play"} title={playback.error || undefined}><Icon name={playback.phase === "playing" ? "pause" : "play"} size={17} /></button><button disabled={playback.queue.length === 0} onClick={() => void playbackController.next()} aria-label={playback.queue.length ? "Play next queued track" : "Next track unavailable: queue is empty"} title="Play the first queued track"><Icon name="next" size={19} /></button><button disabled aria-label="Repeat unavailable" title="Repeat is not implemented"><Icon name="repeat" size={17} /></button></div><div className="seek-row"><span>{formatDuration(playback.currentTime)}</span><input type="range" min="0" max={playback.duration || 1} value={Math.min(playback.currentTime, playback.duration || 0)} disabled={!playback.track || !playback.duration || playback.phase === "error"} onChange={(event) => playbackController.seek(Number(event.target.value))} aria-label="Playback position" /><span>{formatDuration(playback.duration)}</span></div></div>
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
      {playerOpen && <IpodPlayer onClose={() => setPlayerOpen(false)} selectedTrack={selectedTrack} playback={playback} />}
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

function TrackSection({ title, tracks, selectedTrackId, playback, onSelectTrack, onPlay, onAddToQueue, emptyCopy = "No matching tracks were found." }: { title: string; tracks: Track[]; selectedTrackId?: number; playback: PlaybackSnapshot; onSelectTrack: (track: Track) => void; onPlay: (track: Track) => void; onAddToQueue: (track: Track) => void; emptyCopy?: string }) {
  return <section className="content-section track-section"><SectionTitle title={title} />{tracks.length ? <div className="track-list" role="list"><div className="track-header"><span>#</span><span>Track</span><span>Album</span><span>Time</span><span aria-hidden="true" /><span aria-hidden="true" /></div>{tracks.map((track, index) => <TrackRow key={track.id} track={track} index={index} selected={track.id === selectedTrackId} playback={playback} onSelect={onSelectTrack} onPlay={onPlay} onAddToQueue={onAddToQueue} />)}</div> : <EmptyState icon="music" title="No tracks to show" copy={emptyCopy} />}</section>;
}

function CollectionSection({ title, items, emptyTitle }: { title: string; items: { title: string; subtitle: string; shape?: "square" | "circle"; icon: IconName }[]; emptyTitle: string }) {
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
