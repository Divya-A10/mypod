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
const { createIosLibraryAdapter } = await vite.ssrLoadModule("/src/library/iosLibraryAdapter.ts");
const { BaseDirectory } = await vite.ssrLoadModule("@tauri-apps/plugin-fs");
after(async () => vite.close());

const preview = {
  complete: true,
  columns: ["Track name", "Artist name", "Album", "Playlist name", "Type", "ISRC", "Spotify - id"],
  summary: { source_records: 1, preview_rows: 1, ignored_empty_records: 0, ignored_empty_spans: [], rows_with_errors: 0, rows_with_warnings: 0 },
  file_errors: [],
  rows: [],
  duplicate_reports: [],
};

function makeAdapter(overrides = {}) {
  const calls = { invoke: [], open: [], copyFile: [], mkdir: [], remove: [], stat: [] };
  const dependencies = {
    invoke: async (...args) => { calls.invoke.push(args); return preview; },
    open: async (options) => { calls.open.push(options); return "file:///provider/My%20Library.csv"; },
    copyFile: async (...args) => calls.copyFile.push(args),
    mkdir: async (...args) => calls.mkdir.push(args),
    remove: async (...args) => calls.remove.push(args),
    stat: async (...args) => { calls.stat.push(args); return { isFile: true, isSymlink: false, size: 128 }; },
    randomId: () => "0123456789abcdef0123456789abcdef",
    ...overrides,
  };
  return { adapter: createIosLibraryAdapter(dependencies), calls };
}

test("CSV picker cancellation does not copy or invoke the parser", async () => {
  const { adapter, calls } = makeAdapter({ open: async () => null });
  assert.deepEqual(await adapter.previewSpotifyCsv(), { status: "cancelled" });
  assert.equal(calls.copyFile.length, 0);
  assert.equal(calls.invoke.length, 0);
});

test("CSV preview uses a scoped Files picker and token-only native command", async () => {
  const { adapter, calls } = makeAdapter();
  assert.deepEqual(await adapter.previewSpotifyCsv(), { status: "previewed", preview });
  assert.equal(calls.open[0].pickerMode, "document");
  assert.equal(calls.open[0].fileAccessMode, "scoped");
  assert.deepEqual(calls.open[0].filters, [{ name: "Spotify CSV", extensions: ["csv"] }]);
  assert.deepEqual(calls.copyFile[0], [
    "file:///provider/My%20Library.csv",
    "MyPod/Importing/spotify-0123456789abcdef0123456789abcdef.csv.part",
    { toPathBaseDir: BaseDirectory.AppLocalData },
  ]);
  assert.deepEqual(calls.invoke[0], ["preview_spotify_csv", { importId: "0123456789abcdef0123456789abcdef" }]);
  assert.deepEqual(calls.remove[0], [
    "MyPod/Importing/spotify-0123456789abcdef0123456789abcdef.csv.part",
    { baseDir: BaseDirectory.AppLocalData },
  ]);
});

test("CSV preview rejects oversized, symlinked, non-file, and non-CSV selections before copying", async () => {
  const oversized = makeAdapter({ stat: async () => ({ isFile: true, isSymlink: false, size: 10 * 1024 * 1024 + 1 }) });
  await assert.rejects(oversized.adapter.previewSpotifyCsv(), /10 MiB preview limit/);
  assert.equal(oversized.calls.copyFile.length, 0);

  const symlink = makeAdapter({ stat: async () => ({ isFile: true, isSymlink: true, size: 128 }) });
  await assert.rejects(symlink.adapter.previewSpotifyCsv(), /regular file/);
  assert.equal(symlink.calls.copyFile.length, 0);

  const directory = makeAdapter({ stat: async () => ({ isFile: false, isSymlink: false, size: 128 }) });
  await assert.rejects(directory.adapter.previewSpotifyCsv(), /regular file/);
  assert.equal(directory.calls.copyFile.length, 0);

  const wrongExtension = makeAdapter({ open: async () => "file:///provider/export.txt" });
  await assert.rejects(wrongExtension.adapter.previewSpotifyCsv(), /Choose a CSV file/);
  assert.equal(wrongExtension.calls.copyFile.length, 0);
});

test("CSV preview accepts a file reported at the exact byte limit", async () => {
  const { adapter, calls } = makeAdapter({ stat: async () => ({ isFile: true, isSymlink: false, size: 10 * 1024 * 1024 }) });
  assert.equal((await adapter.previewSpotifyCsv()).status, "previewed");
  assert.equal(calls.copyFile.length, 1);
  assert.equal(calls.invoke.length, 1);
});

test("failed native parsing still removes its temporary staged copy", async () => {
  const { adapter, calls } = makeAdapter({ invoke: async () => { throw new Error("invalid CSV"); } });
  await assert.rejects(adapter.previewSpotifyCsv(), /invalid CSV/);
  assert.equal(calls.copyFile.length, 1);
  assert.equal(calls.remove.length, 1);
});
