import { getTrackAudioUrl } from "../api";
import type { PlaybackDriver, PlaybackDriverEvent, PlaybackDriverEventType } from "./PlaybackController";

/** Existing desktop playback implementation backed by HTMLAudioElement. */
export class HtmlAudioPlaybackDriver implements PlaybackDriver {
  private readonly media: HTMLAudioElement;
  private readonly listeners = new Set<(event: PlaybackDriverEvent) => void>();
  private readonly eventTypes: Array<[string, PlaybackDriverEventType]> = [
    ["loadstart", "loadstart"],
    ["loadedmetadata", "metadata"],
    ["durationchange", "metadata"],
    ["timeupdate", "timeupdate"],
    ["seeked", "timeupdate"],
    ["playing", "playing"],
    ["waiting", "waiting"],
    ["pause", "pause"],
    ["ended", "ended"],
    ["error", "error"],
  ];

  constructor(media: HTMLAudioElement = new Audio()) {
    this.media = media;
    this.media.preload = "metadata";
    for (const [sourceType, type] of this.eventTypes) {
      this.media.addEventListener(sourceType, () => this.emit(type));
    }
  }

  get paused() { return this.media.paused; }
  get currentTime() { return this.media.currentTime; }
  get duration() { return this.media.duration; }

  load(trackId: number): void {
    this.media.src = getTrackAudioUrl(trackId);
    this.media.load();
  }

  play(): Promise<void> { return this.media.play(); }
  pause(): void { this.media.pause(); }

  seek(seconds: number): void {
    this.media.currentTime = seconds;
  }

  subscribe(listener: (event: PlaybackDriverEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(type: PlaybackDriverEventType): void {
    const event: PlaybackDriverEvent = {
      type,
      currentTime: this.media.currentTime,
      duration: this.media.duration,
      paused: this.media.paused,
      unsupported: this.media.error?.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED,
    };
    for (const listener of this.listeners) listener(event);
  }
}
