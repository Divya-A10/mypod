export type Track = {
  id: number;
  path: string;
  storage_type: "external" | "managed";
  title: string;
  artist: string | null;
  album: string | null;
  duration: number | null;
  format: string;
  file_size: number;
  modified_at: number;
  created_at: string;
  updated_at: string;
  is_missing: number;
};

type TrackResponse = {
  items: Track[];
  total: number;
  limit: number;
  offset: number;
};

const apiBase = import.meta.env.DEV ? "/api" : "http://127.0.0.1:8000";

export async function getTracks(offset = 0, signal?: AbortSignal): Promise<TrackResponse> {
  const response = await fetch(`${apiBase}/tracks?limit=100&offset=${offset}`, { signal });
  if (!response.ok) {
    throw new Error(`Library request failed (${response.status})`);
  }
  return (await response.json()) as TrackResponse;
}

export function getTrackAudioUrl(trackId: number): string {
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
