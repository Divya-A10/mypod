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
  const calls = { invoke: [], open: [], save: [], copyFile: [], mkdir: [], remove: [], stat: [], startScope: [], stopScope: [] };
  const dependencies = {
    invoke: async (...args) => {
      calls.invoke.push(args);
      return { track: { id: 7, title: "Synthetic tone" }, duplicate: false };
    },
    open: async (options) => {
      calls.open.push(options);
      return "file:///provider/Synthetic%20tone.wav";
    },
    save: async (options) => {
      calls.save.push(options);
      return "file:///provider/MyPod-backup.mypodbackup";
    },
    copyFile: async (...args) => calls.copyFile.push(args),
    mkdir: async (...args) => calls.mkdir.push(args),
    remove: async (...args) => calls.remove.push(args),
    stat: async (...args) => {
      calls.stat.push(args);
      return { isFile: true, isSymlink: false, size: 44 };
    },
    startAccessingSecurityScopedResource: async (...args) => calls.startScope.push(args),
    stopAccessingSecurityScopedResource: async (...args) => calls.stopScope.push(args),
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

test("failed catalog registration preserves the app-private partial copy for recovery", async () => {
  const { adapter, calls } = makeAdapter({ invoke: async () => { throw new Error("catalog unavailable"); } });
  await assert.rejects(adapter.importAudio(), /catalog unavailable/);
  assert.equal(calls.remove.length, 0);
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

test("backup export uses a generated token and preserves its app-private bundle on export failure", async () => {
  const summary = { scope: "complete", schema_version: 5, file_count: 2, sha256: "a".repeat(64) };
  const { adapter, calls } = makeAdapter({
    invoke: async (...args) => { calls.invoke.push(args); return summary; },
    copyFile: async (...args) => { calls.copyFile.push(args); throw new Error("destination unavailable"); },
  });
  const result = await adapter.backups.createBackup("complete");
  assert.equal(result.status, "created");
  assert.equal(result.token, "0123456789abcdef0123456789abcdef");
  assert.match(result.exportError, /destination unavailable/);
  assert.deepEqual(calls.invoke[0], ["create_local_backup", { token: result.token, scope: "complete" }]);
  assert.equal(calls.copyFile[0][0], `MyPod/Backups/${result.token}.mypodbackup`);
  assert.equal(calls.remove.length, 0, "the retained bundle remains available for retry");
  assert.equal(calls.save.length, 1);
});

test("backup and restore cancellation do not call native catalog commands", async () => {
  const { adapter, calls } = makeAdapter({
    save: async () => null,
    open: async () => null,
  });
  assert.deepEqual(await adapter.backups.createBackup("database_only"), { status: "cancelled" });
  assert.deepEqual(await adapter.backups.inspectBackup(), { status: "cancelled" });
  assert.equal(calls.invoke.length, 0);
  assert.equal(calls.copyFile.length, 0);
});

test("retry export reports cancellation and keeps the app-private backup", async () => {
  const { adapter, calls } = makeAdapter({ save: async () => null });
  assert.equal(await adapter.backups.exportBackup("0123456789abcdef0123456789abcdef"), false);
  assert.equal(calls.copyFile.length, 0);
  assert.equal(calls.remove.length, 0);
});

test("restore review stages a Files selection and passes only a generated token to native code", async () => {
  const summary = { scope: "database_only", schema_version: 5, file_count: 1, sha256: "b".repeat(64) };
  const { adapter, calls } = makeAdapter({
    invoke: async (...args) => { calls.invoke.push(args); return summary; },
  });
  const reviewed = await adapter.backups.inspectBackup();
  assert.equal(reviewed.status, "ready");
  assert.deepEqual(calls.invoke[0], ["inspect_local_backup", { token: reviewed.token }]);
  assert.deepEqual(calls.copyFile[0], [
    "file:///provider/Synthetic%20tone.wav",
    `MyPod/Importing/backup-${reviewed.token}.part`,
    { toPathBaseDir: BaseDirectory.AppLocalData },
  ]);
  await adapter.backups.restoreBackup(reviewed.token);
  assert.deepEqual(calls.invoke[1], ["restore_local_backup", { token: reviewed.token, confirmed: true }]);
  assert.ok(!JSON.stringify(calls.invoke).includes("/provider/"), "native commands never receive a user path");
});
