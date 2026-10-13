import { getTrackAudioUrl, type Track } from "../api";

export type PlaybackPhase = "idle" | "loading" | "playing" | "paused" | "ended" | "error";

export type QueueEntry = {
  entryId: number;
  trackId: number;
  track: Track;
};

export type PlaybackHistoryEntry = {
  entryId: number;
  trackId: number;
  track: Track;
};

export type PlaybackSnapshot = {
  /** The media item being loaded, playing, paused, ended, or in an error state. */
  track: Track | null;
  phase: PlaybackPhase;
  currentTime: number;
  duration: number | null;
  error: string | null;
  /** Upcoming items only. The current item remains separate. */
  queue: QueueEntry[];
  /** Successfully started tracks, oldest first. */
  history: PlaybackHistoryEntry[];
};

type Listener = (snapshot: PlaybackSnapshot) => void;

/** UI-independent in-memory queue and playback history, keyed by library track IDs. */
export class PlaybackQueue {
  private nextEntryId = 1;
  private upcoming: QueueEntry[] = [];
  private played: PlaybackHistoryEntry[] = [];

  getUpcoming = (): QueueEntry[] => this.upcoming.map((entry) => ({ ...entry }));

  getHistory = (): PlaybackHistoryEntry[] => this.played.map((entry) => ({ ...entry }));

  enqueue = (track: Track): QueueEntry => {
    const entry = { entryId: this.nextEntryId++, trackId: track.id, track };
    this.upcoming = [...this.upcoming, entry];
    return entry;
  };

  enqueueFirst = (track: Track): QueueEntry => {
    const entry = { entryId: this.nextEntryId++, trackId: track.id, track };
    this.upcoming = [entry, ...this.upcoming];
    return entry;
  };

  peekNext = (): QueueEntry | null => this.upcoming[0] ?? null;

  remove = (entryId: number): void => {
    this.upcoming = this.upcoming.filter((entry) => entry.entryId !== entryId);
  };

  clear = (): void => {
    this.upcoming = [];
  };

  recordQueueStart = (entryId: number, previouslyStarted: Track | null): boolean => {
    const index = this.upcoming.findIndex((entry) => entry.entryId === entryId);
    if (index < 0) return false;
    this.upcoming = this.upcoming.filter((entry) => entry.entryId !== entryId);
    this.recordHistory(previouslyStarted);
    return true;
  };

  recordDirectStart = (previouslyStarted: Track | null): void => {
    this.recordHistory(previouslyStarted);
  };

  peekPrevious = (): PlaybackHistoryEntry | null => this.played.at(-1) ?? null;

  recordPreviousStart = (historyEntryId: number, previouslyStarted: Track | null): boolean => {
    const index = this.played.findIndex((entry) => entry.entryId === historyEntryId);
    if (index < 0) return false;
    this.played = this.played.filter((entry) => entry.entryId !== historyEntryId);
    if (previouslyStarted) this.enqueueFirst(previouslyStarted);
    return true;
  };

  private recordHistory(track: Track | null): void {
    if (!track) return;
    this.played = [...this.played, { entryId: this.nextEntryId++, trackId: track.id, track }];
  }
}

type StartRequest =
  | { kind: "resume"; track: Track }
  | { kind: "direct"; track: Track; previouslyStarted: Track | null }
  | { kind: "queue"; track: Track; queueEntryId: number; previouslyStarted: Track | null }
  | { kind: "previous"; track: Track; historyEntryId: number; previouslyStarted: Track | null };
type StartIntent = StartRequest & { attempt: number };

/** One media element and one observable state source for every player surface. */
export class PlaybackController {
  private readonly media: HTMLAudioElement;
  private readonly queueModel: PlaybackQueue;
  private readonly listeners = new Set<Listener>();
  private playAttempt = 0;
  private startedTrack: Track | null = null;
  private startIntent: StartIntent | null = null;
  private snapshot: PlaybackSnapshot = {
    track: null,
    phase: "idle",
    currentTime: 0,
    duration: null,
    error: null,
    queue: [],
    history: [],
  };

  constructor(media: HTMLAudioElement = new Audio(), queueModel = new PlaybackQueue()) {
    this.media = media;
    this.queueModel = queueModel;
    this.media.preload = "metadata";
    this.media.addEventListener("loadstart", this.onLoadStart);
    this.media.addEventListener("loadedmetadata", this.onMetadata);
    this.media.addEventListener("timeupdate", this.onTimeUpdate);
    this.media.addEventListener("durationchange", this.onMetadata);
    this.media.addEventListener("playing", this.onPlaying);
    this.media.addEventListener("waiting", this.onWaiting);
    this.media.addEventListener("pause", this.onPause);
    this.media.addEventListener("ended", this.onEnded);
    this.media.addEventListener("error", this.onError);
    this.media.addEventListener("seeked", this.onTimeUpdate);
  }

  getSnapshot = (): PlaybackSnapshot => this.snapshot;

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    listener(this.snapshot);
    return () => this.listeners.delete(listener);
  };

  enqueue = (track: Track): QueueEntry => {
    const entry = this.queueModel.enqueue(track);
    this.syncQueue();
    return entry;
  };

  removeFromQueue = (entryId: number): void => {
    this.queueModel.remove(entryId);
    this.syncQueue();
  };

  clearQueue = (): void => {
    this.queueModel.clear();
    this.syncQueue();
  };

  play = async (track: Track): Promise<void> => {
    const sameTrack = this.snapshot.track?.id === track.id;
    if (sameTrack && !this.media.paused) {
      this.pause();
      return;
    }

    const alreadyStarted = this.startedTrack?.id === track.id;
    const intent = alreadyStarted
      ? { kind: "resume" as const, track }
      : { kind: "direct" as const, track, previouslyStarted: this.startedTrack };
    const restart = this.snapshot.phase === "ended" || this.snapshot.phase === "error" || (this.media.duration > 0 && this.media.currentTime >= this.media.duration);
    await this.startTrack(track, intent, !sameTrack || !alreadyStarted || restart);
  };

  next = async (): Promise<void> => {
    const entry = this.queueModel.peekNext();
    if (!entry) return;
    await this.startTrack(entry.track, {
      kind: "queue",
      track: entry.track,
      queueEntryId: entry.entryId,
      previouslyStarted: this.startedTrack,
    }, true);
  };

  previous = async (): Promise<void> => {
    const current = this.snapshot.track;
    if (!current) return;

    if (this.snapshot.currentTime > 3 || !this.queueModel.peekPrevious()) {
      if (this.snapshot.phase === "ended" || this.snapshot.phase === "error") {
        const intent = this.startedTrack?.id === current.id
          ? { kind: "resume" as const, track: current }
          : { kind: "direct" as const, track: current, previouslyStarted: this.startedTrack };
        await this.startTrack(current, intent, true);
      } else {
        this.seek(0);
      }
      return;
    }

    const entry = this.queueModel.peekPrevious();
    if (!entry) return;
    await this.startTrack(entry.track, {
      kind: "previous",
      track: entry.track,
      historyEntryId: entry.entryId,
      previouslyStarted: this.startedTrack,
    }, true);
  };

  pause = (): void => {
    this.playAttempt += 1;
    this.startIntent = null;
    this.media.pause();
    if (this.media.paused && this.snapshot.track && this.snapshot.phase !== "ended") {
      this.update({ phase: "paused" });
    }
  };

  toggle = (track?: Track | null): void => {
    if (this.snapshot.phase === "loading") {
      this.pause();
      return;
    }
    if (!this.media.paused) {
      this.pause();
      return;
    }
    const target = track ?? this.snapshot.track;
    if (target) void this.play(target);
  };

  seek = (seconds: number): void => {
    if (!this.snapshot.track || !Number.isFinite(seconds) || !Number.isFinite(this.media.duration) || this.media.duration <= 0) return;
    this.media.currentTime = Math.min(Math.max(seconds, 0), this.media.duration);
  };

  private async startTrack(track: Track, intent: StartRequest, reload: boolean): Promise<void> {
    const attempt = ++this.playAttempt;
    this.startIntent = { ...intent, attempt } as StartIntent;
    if (reload) {
      this.media.pause();
      this.snapshot = { ...this.snapshot, track, phase: "loading", currentTime: 0, duration: null, error: null };
      this.emit();
      this.media.src = getTrackAudioUrl(track.id);
      this.media.load();
    } else {
      this.update({ track, phase: "loading", error: null });
    }

    try {
      await this.media.play();
      // The playing event, not the play() promise, commits queue and history state.
    } catch (reason) {
      if (attempt !== this.playAttempt || this.snapshot.track?.id !== track.id) return;
      this.startIntent = null;
      const name = reason instanceof DOMException ? reason.name : "PlaybackError";
      this.update({
        phase: "error",
        error: name === "NotAllowedError"
          ? "Playback was blocked by the WebView. Try pressing play again."
          : "This track could not be played. It may be unsupported or unavailable.",
      });
    }
  }

  private syncQueue(): void {
    this.update({ queue: this.queueModel.getUpcoming(), history: this.queueModel.getHistory() });
  }

  private update(patch: Partial<PlaybackSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...patch };
    this.emit();
  }

  private emit(): void {
    for (const listener of this.listeners) listener(this.snapshot);
  }

  private onLoadStart = (): void => {
    if (this.snapshot.track && this.snapshot.phase !== "error") this.update({ phase: "loading" });
  };

  private onMetadata = (): void => {
    const duration = Number.isFinite(this.media.duration) ? this.media.duration : null;
    this.update({ duration, currentTime: Number.isFinite(this.media.currentTime) ? this.media.currentTime : 0 });
  };

  private onTimeUpdate = (): void => {
    this.update({ currentTime: Number.isFinite(this.media.currentTime) ? this.media.currentTime : 0 });
  };

  private onPlaying = (): void => {
    const intent = this.startIntent;
    if (!intent || intent.attempt !== this.playAttempt || this.snapshot.track?.id !== intent.track.id) return;

    if (intent.kind === "queue") {
      if (!this.queueModel.recordQueueStart(intent.queueEntryId, intent.previouslyStarted)) {
        this.queueModel.recordDirectStart(intent.previouslyStarted);
      }
    } else if (intent.kind === "direct") {
      if (intent.previouslyStarted?.id !== intent.track.id) this.queueModel.recordDirectStart(intent.previouslyStarted);
    } else if (intent.kind === "previous") {
      this.queueModel.recordPreviousStart(intent.historyEntryId, intent.previouslyStarted);
    }

    this.startedTrack = intent.track;
    this.startIntent = null;
    this.snapshot = {
      ...this.snapshot,
      phase: "playing",
      error: null,
      queue: this.queueModel.getUpcoming(),
      history: this.queueModel.getHistory(),
    };
    this.emit();
  };

  private onWaiting = (): void => {
    if (this.snapshot.track && !this.media.paused) this.update({ phase: "loading" });
  };

  private onPause = (): void => {
    if (this.snapshot.track && this.snapshot.phase !== "loading" && this.snapshot.phase !== "error" && this.snapshot.phase !== "ended") {
      this.update({ phase: "paused" });
    }
  };

  private onEnded = (): void => {
    if (!this.snapshot.track) return;
    const duration = Number.isFinite(this.media.duration) ? this.media.duration : this.snapshot.duration;
    this.update({ phase: "ended", currentTime: duration ?? this.media.currentTime, duration });
    if (this.queueModel.peekNext()) void this.next();
  };

  private onError = (): void => {
    if (!this.snapshot.track) return;
    this.startIntent = null;
    const unsupported = this.media.error?.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED;
    this.update({
      phase: "error",
      error: unsupported
        ? "This audio format is not supported by the current player."
        : "This track could not be loaded. Check that the local API and file are available.",
    });
  };
}

export const playbackController = new PlaybackController();
