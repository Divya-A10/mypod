import { invoke } from "@tauri-apps/api/core";
import type { PlaybackDriver, PlaybackDriverEvent, PlaybackDriverEventType } from "./PlaybackController";

type NativePlaybackSnapshot = {
  track_id: number | null;
  current_time: number;
  duration: number | null;
  playing: boolean;
  loading: boolean;
  ended: boolean;
  error: string | null;
};

type Dependencies = {
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  setInterval: typeof globalThis.setInterval;
  clearInterval: typeof globalThis.clearInterval;
};

const defaults: Dependencies = { invoke, setInterval, clearInterval };

/** iOS playback talks only to native commands, which resolve library IDs privately. */
export class IosNativePlaybackDriver implements PlaybackDriver {
  private readonly listeners = new Set<(event: PlaybackDriverEvent) => void>();
  private trackId: number | null = null;
  private snapshot: NativePlaybackSnapshot | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private polling = false;

  constructor(private readonly dependencies: Dependencies = defaults) {}

  get paused(): boolean { return !this.snapshot?.playing; }
  get currentTime(): number { return this.snapshot?.current_time ?? 0; }
  get duration(): number { return this.snapshot?.duration ?? Number.NaN; }

  load(trackId: number): void {
    this.stopPolling();
    this.trackId = trackId;
    this.snapshot = null;
    this.emit("loadstart");
  }

  async play(): Promise<void> {
    if (!this.trackId || !Number.isSafeInteger(this.trackId) || this.trackId < 1) {
      throw new Error("A valid library track ID is required");
    }
    await this.dependencies.invoke<void>("play_local_track", { trackId: this.trackId });
    this.startPolling();
    await this.poll();
  }

  pause(): void {
    void this.dependencies.invoke<void>("pause_local_playback")
      .then(() => this.startPolling())
      .catch(() => this.fail("Native playback could not be paused."));
  }

  seek(seconds: number): void {
    if (!Number.isFinite(seconds) || seconds < 0) return;
    void this.dependencies.invoke<void>("seek_local_playback", { seconds })
      .then(() => this.poll())
      .catch(() => this.fail("Native playback could not seek to that position."));
  }

  subscribe(listener: (event: PlaybackDriverEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private startPolling(): void {
    if (this.timer !== null) return;
    this.timer = this.dependencies.setInterval(() => { void this.poll(); }, 250);
    void this.poll();
  }

  private stopPolling(): void {
    if (this.timer !== null) this.dependencies.clearInterval(this.timer);
    this.timer = null;
    this.polling = false;
  }

  private async poll(): Promise<void> {
    if (this.polling || this.trackId === null) return;
    this.polling = true;
    try {
      const next = await this.dependencies.invoke<NativePlaybackSnapshot>("local_playback_state");
      if (next.track_id !== this.trackId) return;
      const previous = this.snapshot;
      this.snapshot = next;

      if (next.duration !== previous?.duration) this.emit("metadata");
      if (next.current_time !== previous?.current_time) this.emit("timeupdate");
      if (next.error && !previous?.error) {
        this.emit("error", true);
        this.stopPolling();
      } else if (next.ended && !previous?.ended) {
        this.emit("ended");
        this.stopPolling();
      } else if (next.playing && !previous?.playing) {
        this.emit("playing");
      } else if (previous?.playing && !next.playing) {
        this.emit("pause");
      } else if (next.loading && !previous?.loading) {
        this.emit("waiting");
      }
    } catch {
      this.fail("Native playback state is unavailable.");
    } finally {
      this.polling = false;
    }
  }

  private fail(message: string): void {
    if (this.snapshot?.error) return;
    this.snapshot = {
      ...this.snapshot,
      track_id: this.trackId,
      current_time: this.currentTime,
      duration: this.snapshot?.duration ?? null,
      playing: false,
      loading: false,
      ended: false,
      error: message,
    };
    this.emit("error");
    this.stopPolling();
  }

  private emit(type: PlaybackDriverEventType, unsupported = false): void {
    const event: PlaybackDriverEvent = {
      type,
      currentTime: this.currentTime,
      duration: this.duration,
      paused: this.paused,
      unsupported,
    };
    for (const listener of this.listeners) listener(event);
  }
}
