import assert from "node:assert/strict";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

const vite = await createServer({
  configFile: fileURLToPath(new URL("../vite.config.ts", import.meta.url)),
  appType: "custom",
  logLevel: "silent",
  server: { middlewareMode: true },
});
const { createDesktopLibraryAdapter } = await vite.ssrLoadModule("/src/library/desktopLibraryAdapter.ts");
const { MAX_SPOTIFY_CSV_BYTES, getTrackArtwork, previewSpotifyCsv } = await vite.ssrLoadModule("/src/api.ts");
after(async () => vite.close());

test("desktop/web adapter exposes preview only and does not offer iOS commands", () => {
  const adapter = createDesktopLibraryAdapter({
    isTauri: () => false,
    openDirectory: async () => { throw new Error("must not open native dialog in browser"); },
    listTracks: async () => ({ items: [], total: 0, limit: 100, offset: 0 }),
    getTrack: async () => { throw new Error("unused"); },
    previewSpotifyCsv: async () => ({ complete: true, columns: [], summary: { source_records: 0, preview_rows: 0, ignored_empty_records: 0, ignored_empty_spans: [], rows_with_errors: 0, rows_with_warnings: 0 }, file_errors: [], rows: [], duplicate_reports: [] }),
    scanAudioFolder: async () => { throw new Error("must not scan in browser"); },
  });
  assert.equal(typeof adapter.previewSpotifyCsv, "function");
  assert.equal(adapter.scanAudioFolder, undefined);
  assert.equal(adapter.confirmSpotifyImport, undefined);
});

test("Tauri desktop scans only the user-selected folder and reports cancellation", async () => {
  const calls = [];
  const adapter = createDesktopLibraryAdapter({
    isTauri: () => true,
    openDirectory: async () => "/permitted/synthetic",
    listTracks: async () => ({ items: [], total: 0, limit: 100, offset: 0 }),
    getTrack: async () => { throw new Error("unused"); },
    previewSpotifyCsv: async () => { throw new Error("unused"); },
    scanAudioFolder: async (directory) => { calls.push(directory); return { imported: 2, updated: 1, skipped: 3, failed: 0, missing: 0 }; },
  });
  assert.deepEqual(await adapter.scanAudioFolder(), {
    status: "scanned",
    result: { imported: 2, updated: 1, skipped: 3, failed: 0, missing: 0 },
  });
  assert.deepEqual(calls, ["/permitted/synthetic"]);

  const cancelled = createDesktopLibraryAdapter({
    isTauri: () => true,
    openDirectory: async () => null,
    listTracks: async () => ({ items: [], total: 0, limit: 100, offset: 0 }),
    getTrack: async () => { throw new Error("unused"); },
    previewSpotifyCsv: async () => { throw new Error("unused"); },
    scanAudioFolder: async () => { throw new Error("cancelled picker must not scan"); },
  });
  assert.deepEqual(await cancelled.scanAudioFolder(), { status: "cancelled" });
});

test("Spotify CSV preview posts the chosen bytes to the existing preview-only endpoint", async () => {
  const originalFetch = globalThis.fetch;
  const calls = [];
  const preview = { complete: true, columns: ["Track name"], summary: {}, file_errors: [], rows: [], duplicate_reports: [] };
  globalThis.fetch = async (...args) => {
    calls.push(args);
    return new Response(JSON.stringify(preview), { status: 200, headers: { "Content-Type": "application/json" } });
  };
  try {
    const file = { name: "synthetic.csv", size: 8, arrayBuffer: async () => new Uint8Array([1, 2, 3]).buffer };
    assert.deepEqual(await previewSpotifyCsv(file), preview);
    assert.equal(calls.length, 1);
    assert.equal(calls[0][0], "/api/imports/spotify/preview");
    assert.equal(calls[0][1].method, "POST");
    assert.equal(calls[0][1].headers["Content-Type"], "text/csv; charset=utf-8");
    assert.equal(calls[0][1].body, file);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("CSV preview rejects unsupported extensions and oversized files before network access", async () => {
  const originalFetch = globalThis.fetch;
  let called = false;
  globalThis.fetch = async () => { called = true; throw new Error("unexpected network request"); };
  try {
    await assert.rejects(previewSpotifyCsv({ name: "not-csv.txt", size: 1 }), /Choose a CSV/);
    await assert.rejects(previewSpotifyCsv({ name: "large.csv", size: MAX_SPOTIFY_CSV_BYTES + 1 }), /10 MiB/);
    assert.equal(called, false);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("desktop artwork request uses a library track ID rather than a filesystem path", async () => {
  const originalFetch = globalThis.fetch;
  const calls = [];
  globalThis.fetch = async (...args) => {
    calls.push(args);
    return new Response(new Uint8Array([1, 2, 3]), { status: 200, headers: { "Content-Type": "image/png" } });
  };
  try {
    const blob = await getTrackArtwork(19);
    assert.equal(blob.type, "image/png");
    assert.deepEqual([...new Uint8Array(await blob.arrayBuffer())], [1, 2, 3]);
    assert.equal(calls[0][0], "/api/tracks/19/artwork");
    await assert.rejects(getTrackArtwork(0), /valid library track ID/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
