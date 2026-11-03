import { assertDesktopAdapterAvailable } from "./platform";
import type { FolderScanResult, SpotifyCsvPreview, Track, TrackResponse } from "./library/contracts";

export type { LibraryAdapter, Track, TrackResponse } from "./library/contracts";

const apiBase = import.meta.env.DEV ? "/api" : "http://127.0.0.1:8000";
export const MAX_SPOTIFY_CSV_BYTES = 10 * 1024 * 1024;

export async function previewSpotifyCsv(file: File): Promise<SpotifyCsvPreview> {
  assertDesktopAdapterAvailable();
  if (!file.name.toLocaleLowerCase().endsWith(".csv")) {
    throw new Error("Choose a CSV file.");
  }
  if (file.size > MAX_SPOTIFY_CSV_BYTES) {
    throw new Error(`CSV exceeds the 10 MiB preview limit (${MAX_SPOTIFY_CSV_BYTES} bytes).`);
  }
  const response = await fetch(`${apiBase}/imports/spotify/preview`, {
    method: "POST",
    headers: { "Content-Type": "text/csv; charset=utf-8" },
    body: file,
  });
  if (!response.ok) {
    const detail = await response.json().catch(() => null) as { detail?: { code?: string } | string } | null;
    const code = typeof detail?.detail === "object" ? detail.detail?.code : undefined;
    if (response.status === 413 || code === "file_too_large") {
      throw new Error("CSV exceeds the 10 MiB preview limit.");
    }
    throw new Error(`CSV preview request failed (${response.status}).`);
  }
  return await response.json() as SpotifyCsvPreview;
}

export async function scanAudioFolder(directory: string): Promise<FolderScanResult> {
  assertDesktopAdapterAvailable();
  const response = await fetch(`${apiBase}/scan`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ directory }),
  });
  if (!response.ok) {
    const detail = await response.json().catch(() => null) as { detail?: string } | null;
    throw new Error(detail?.detail || `Folder scan failed (${response.status}).`);
  }
  return await response.json() as FolderScanResult;
}

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

export async function getTrackArtwork(trackId: number): Promise<Blob | null> {
  assertDesktopAdapterAvailable();
  if (!Number.isSafeInteger(trackId) || trackId < 1) {
    throw new Error("A valid library track ID is required");
  }
  const response = await fetch(`${apiBase}/tracks/${trackId}/artwork`);
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(`Artwork request failed (${response.status}).`);
  const blob = await response.blob();
  if (blob.size <= 0 || blob.size > 8 * 1024 * 1024 || !blob.type.startsWith("image/")) {
    throw new Error("The library returned unsupported artwork.");
  }
  return blob;
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
