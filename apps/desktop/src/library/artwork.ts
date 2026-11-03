import { libraryAdapter } from "./libraryAdapter";

const MAX_THUMBNAIL_SIDE = 768;
const MAX_SOURCE_DIMENSION = 8192;
const MAX_SOURCE_PIXELS = 12_000_000;
const MAX_CACHED_ARTWORK = 48;
const MAX_CONCURRENT_ARTWORK = 2;

const thumbnails = new Map<number, string | null>();
const pending = new Map<number, Promise<string | null>>();
let activeArtworkRequests = 0;
const artworkWaiters: Array<() => void> = [];

async function withArtworkSlot<T>(operation: () => Promise<T>): Promise<T> {
  if (activeArtworkRequests >= MAX_CONCURRENT_ARTWORK) {
    await new Promise<void>((resolve) => artworkWaiters.push(resolve));
  }
  activeArtworkRequests += 1;
  try {
    return await operation();
  } finally {
    activeArtworkRequests -= 1;
    artworkWaiters.shift()?.();
  }
}

function remember(trackId: number, value: string | null): void {
  const previous = thumbnails.get(trackId);
  if (previous && previous !== value) URL.revokeObjectURL(previous);
  thumbnails.delete(trackId);
  thumbnails.set(trackId, value);
  while (thumbnails.size > MAX_CACHED_ARTWORK) {
    const oldestId = thumbnails.keys().next().value as number | undefined;
    if (oldestId === undefined) break;
    const url = thumbnails.get(oldestId);
    if (url) URL.revokeObjectURL(url);
    thumbnails.delete(oldestId);
  }
}

async function decode(blob: Blob): Promise<ImageBitmap | HTMLImageElement> {
  if (typeof createImageBitmap === "function") return createImageBitmap(blob);
  const url = URL.createObjectURL(blob);
  try {
    const image = new Image();
    image.src = url;
    await image.decode();
    return image;
  } finally {
    URL.revokeObjectURL(url);
  }
}

async function resizeArtwork(blob: Blob): Promise<Blob> {
  const image = await decode(blob);
  try {
    const width = image.width;
    const height = image.height;
    if (!width || !height || width > MAX_SOURCE_DIMENSION || height > MAX_SOURCE_DIMENSION || width * height > MAX_SOURCE_PIXELS) {
      throw new Error("Artwork dimensions exceed the safe display limit.");
    }
    const scale = Math.min(1, MAX_THUMBNAIL_SIDE / Math.max(width, height));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(width * scale));
    canvas.height = Math.max(1, Math.round(height * scale));
    const context = canvas.getContext("2d", { alpha: false });
    if (!context) throw new Error("Artwork could not be prepared for display.");
    context.drawImage(image, 0, 0, canvas.width, canvas.height);
    const thumbnail = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.9));
    return thumbnail ?? blob;
  } finally {
    if ("close" in image && typeof image.close === "function") image.close();
  }
}

/** Load embedded track art by stable library ID; paths remain inside adapters. */
export function loadTrackArtwork(trackId: number): Promise<string | null> {
  const cached = thumbnails.get(trackId);
  if (cached !== undefined) {
    thumbnails.delete(trackId);
    thumbnails.set(trackId, cached);
    return Promise.resolve(cached);
  }
  const inFlight = pending.get(trackId);
  if (inFlight) return inFlight;

  const request = (async () => {
    try {
      const thumbnail = await withArtworkSlot(async () => {
        const source = await libraryAdapter.getTrackArtwork?.(trackId) ?? null;
        return source ? resizeArtwork(source) : null;
      });
      const url = thumbnail ? URL.createObjectURL(thumbnail) : null;
      remember(trackId, url);
      return url;
    } catch {
      remember(trackId, null);
      return null;
    } finally {
      pending.delete(trackId);
    }
  })();
  pending.set(trackId, request);
  return request;
}
