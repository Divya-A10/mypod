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
  ready?: boolean;
  player_status?: string;
  item_status?: string;
  time_control_status?: string;
  failure_code?: number | null;
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
  private reloadRequested = false;
  private snapshot: NativePlaybackSnapshot | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private polling = false;
  private lastDiagnosticState: string | null = null;

  constructor(private readonly dependencies: Dependencies = defaults) {}

  get paused(): boolean { return !this.snapshot?.playing; }
  get currentTime(): number { return this.snapshot?.current_time ?? 0; }
  get duration(): number { return this.snapshot?.duration ?? Number.NaN; }

  load(trackId: number): void {
    this.stopPolling();
    this.trackId = trackId;
    this.reloadRequested = true;
    this.snapshot = null;
    this.emit("loadstart");
  }

  async play(): Promise<void> {
    if (!this.trackId || !Number.isSafeInteger(this.trackId) || this.trackId < 1) {
      throw new Error("A valid library track ID is required");
    }
    console.debug("MyPod iOS playback: dispatching native play command");
    try {
      await this.dependencies.invoke<void>("play_local_track", {
        trackId: this.trackId,
        reload: this.reloadRequested,
      });
      this.reloadRequested = false;
      console.info("MyPod iOS playback: native play command accepted");
    } catch {
      console.warn("MyPod iOS playback: native play command failed");
      throw new Error("The iPhone could not start this track.");
    }
    this.startPolling();
    await this.poll();
  }

  pause(): void {
    void this.dependencies.invoke<void>("pause_local_playback")
      .then(() => this.startPolling())
      .catch(() => {
        console.warn("MyPod iOS playback: native pause command failed");
        this.fail("Native playback could not be paused.");
      });
  }

  seek(seconds: number): void {
    if (!Number.isFinite(seconds) || seconds < 0) return;
    void this.dependencies.invoke<void>("seek_local_playback", { seconds })
      .then(() => this.poll())
      .catch(() => {
        console.warn("MyPod iOS playback: native seek command failed");
        this.fail("Native playback could not seek to that position.");
      });
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
      if (next.track_id !== this.trackId) {
        console.warn("MyPod iOS playback: native state track did not match the requested track");
        return;
      }
      const previous = this.snapshot;
      this.snapshot = next;
      const diagnosticState = [next.player_status, next.item_status, next.time_control_status, next.ready, next.duration !== null, next.playing, next.loading, Boolean(next.error), next.failure_code ?? ""].join("|");
      if (diagnosticState !== this.lastDiagnosticState) {
        console.info("MyPod iOS playback: native state", {
          playerStatus: next.player_status ?? "unknown",
          itemStatus: next.item_status ?? "unknown",
          timeControlStatus: next.time_control_status ?? "unknown",
          ready: Boolean(next.ready),
          durationAvailable: next.duration !== null,
          playing: next.playing,
          loading: next.loading,
          failed: Boolean(next.error),
          failureCode: next.failure_code ?? null,
        });
        this.lastDiagnosticState = diagnosticState;
      }

      if (next.duration !== previous?.duration) this.emit("metadata");
      if (next.current_time !== previous?.current_time) this.emit("timeupdate");
      if (next.error && !previous?.error) {
        // AVPlayer failure alone does not prove that the codec is unsupported.
        console.error("MyPod iOS playback: native media failure", { failureCode: next.failure_code ?? null });
        const message = next.failure_code === null || next.failure_code === undefined
          ? "iOS could not load this audio file. Try another file or re-import it."
          : `iOS could not load this audio file (AVFoundation error ${next.failure_code}).`;
        this.emit("error", false, message);
        this.stopPolling();
      } else if (next.ended && !previous?.ended) {
        this.emit("ended");
        this.stopPolling();
      } else if (next.playing && !previous?.playing) {
        console.info("MyPod iOS playback: native playback confirmed");
        this.emit("playing");
      } else if (previous?.playing && !next.playing) {
        this.emit("pause");
      } else if (next.loading && !previous?.loading) {
        this.emit("waiting");
      }
    } catch {
      console.warn("MyPod iOS playback: native state query failed");
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
    console.error("MyPod iOS playback: native command or state query failed");
    this.emit("error", false, message);
    this.stopPolling();
  }

  private emit(type: PlaybackDriverEventType, unsupported = false, message?: string): void {
    const event: PlaybackDriverEvent = {
      type,
      currentTime: this.currentTime,
      duration: this.duration,
      paused: this.paused,
      unsupported,
      ...(message ? { message } : {}),
    };
    for (const listener of this.listeners) listener(event);
  }
}
