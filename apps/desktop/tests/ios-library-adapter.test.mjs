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

function makeAdapter(overrides = {}) {
  const calls = { invoke: [], open: [], copyFile: [], mkdir: [], remove: [], stat: [] };
  const dependencies = {
    invoke: async (...args) => {
      calls.invoke.push(args);
      return { track: { id: 7, title: "Synthetic tone" }, duplicate: false };
    },
    open: async (options) => {
      calls.open.push(options);
      return "file:///provider/Synthetic%20tone.wav";
    },
    copyFile: async (...args) => calls.copyFile.push(args),
    mkdir: async (...args) => calls.mkdir.push(args),
    remove: async (...args) => calls.remove.push(args),
    stat: async (...args) => {
      calls.stat.push(args);
      return { isFile: true, isSymlink: false, size: 44 };
    },
    randomId: () => "0123456789abcdef0123456789abcdef",
    ...overrides,
  };
  return { adapter: createIosLibraryAdapter(dependencies), calls };
}

test("iOS import cancellation is a no-op", async () => {
  const { adapter, calls } = makeAdapter({ open: async () => null });
  assert.deepEqual(await adapter.importAudio(), { status: "cancelled" });
  assert.equal(calls.copyFile.length, 0);
  assert.equal(calls.invoke.length, 0);
});

test("iOS import copies to a generated app-private staging path before catalog registration", async () => {
  const { adapter, calls } = makeAdapter();
  const result = await adapter.importAudio();
  assert.deepEqual(result, { status: "imported", track: { id: 7, title: "Synthetic tone" } });
  assert.equal(calls.open[0].pickerMode, "document");
  assert.equal(calls.open[0].fileAccessMode, "scoped");
  assert.deepEqual(calls.copyFile[0], [
    "file:///provider/Synthetic%20tone.wav",
    "MyPod/Importing/0123456789abcdef0123456789abcdef.part",
    { toPathBaseDir: BaseDirectory.AppLocalData },
  ]);
  assert.deepEqual(calls.invoke[0], ["register_imported_track", {
    importId: "0123456789abcdef0123456789abcdef",
    originalName: "Synthetic tone.wav",
    extension: "wav",
  }]);
  assert.equal(calls.remove.length, 0);
});

test("iOS adapter reports a native duplicate without changing its track identity", async () => {
  const track = { id: 7, title: "Existing synthetic tone" };
  const { adapter } = makeAdapter({ invoke: async () => ({ track, duplicate: true }) });
  assert.deepEqual(await adapter.importAudio(), { status: "duplicate", track });
});

test("iOS import rejects unsupported locations, unsupported files, and non-regular files", async () => {
  const external = makeAdapter({ open: async () => "https://example.test/audio.wav" });
  await assert.rejects(external.adapter.importAudio(), /unsupported location/);

  const unsupported = makeAdapter({ open: async () => "file:///provider/document.pdf" });
  await assert.rejects(unsupported.adapter.importAudio(), /supported audio file/);

  const link = makeAdapter({ stat: async () => ({ isFile: true, isSymlink: true, size: 44 }) });
  await assert.rejects(link.adapter.importAudio(), /regular file/);
  assert.equal(link.calls.copyFile.length, 0);
});

test("failed catalog registration removes the app-private partial copy", async () => {
  const { adapter, calls } = makeAdapter({ invoke: async () => { throw new Error("catalog unavailable"); } });
  await assert.rejects(adapter.importAudio(), /catalog unavailable/);
  assert.deepEqual(calls.remove[0], [
    "MyPod/Importing/0123456789abcdef0123456789abcdef.part",
    { baseDir: BaseDirectory.AppLocalData },
  ]);
});

test("iOS playlist adapter uses native catalog commands and preserves entry identifiers", async () => {
  const { adapter, calls } = makeAdapter();
  const playlists = adapter.playlists;
  assert.ok(playlists);
  await playlists.listPlaylists();
  await playlists.createPlaylist("Same name");
  await playlists.renamePlaylist(4, "Renamed");
  await playlists.listEntries(4);
  await playlists.addTrack(4, 12);
  await playlists.removeEntry(4, 87);
  await playlists.reorderEntries(4, [91, 87]);
  await playlists.deletePlaylist(4);
  assert.deepEqual(calls.invoke.map(([command, args]) => [command, args]), [
    ["list_playlists", undefined],
    ["create_playlist", { name: "Same name" }],
    ["rename_playlist", { playlistId: 4, name: "Renamed" }],
    ["list_playlist_entries", { playlistId: 4 }],
    ["add_track_to_playlist", { playlistId: 4, trackId: 12 }],
    ["remove_playlist_entry", { playlistId: 4, entryId: 87 }],
    ["reorder_playlist_entries", { playlistId: 4, entryIds: [91, 87] }],
    ["delete_playlist", { playlistId: 4 }],
  ]);
});
