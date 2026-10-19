import { assertDesktopAdapterAvailable } from "./platform";
import type { Track, TrackResponse } from "./library/contracts";

export type { LibraryAdapter, Track, TrackResponse } from "./library/contracts";

const apiBase = import.meta.env.DEV ? "/api" : "http://127.0.0.1:8000";

export async function getTracks(offset = 0, signal?: AbortSignal): Promise<TrackResponse> {
  assertDesktopAdapterAvailable();
  const response = await fetch(`${apiBase}/tracks?limit=100&offset=${offset}`, { signal });
  if (!response.ok) {
    throw new Error(`Library request failed (${response.status})`);
  }
  return (await response.json()) as TrackResponse;
}

export async function getTrack(trackId: number, signal?: AbortSignal): Promise<Track> {
  assertDesktopAdapterAvailable();
  if (!Number.isSafeInteger(trackId) || trackId < 1) {
    throw new Error("A valid library track ID is required");
  }
  const response = await fetch(`${apiBase}/tracks/${trackId}`, { signal });
  if (!response.ok) {
    throw new Error(`Track request failed (${response.status})`);
  }
  return (await response.json()) as Track;
}

export function getTrackAudioUrl(trackId: number): string {
  assertDesktopAdapterAvailable();
  if (!Number.isSafeInteger(trackId) || trackId < 1) {
    throw new Error("A valid library track ID is required");
  }
  return `${apiBase}/tracks/${trackId}/audio`;
}

export function formatDuration(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds) || seconds < 0) return "—";
  const whole = Math.floor(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}
