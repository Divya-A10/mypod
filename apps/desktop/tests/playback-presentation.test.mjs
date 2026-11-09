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
const { playPausePresentation } = await vite.ssrLoadModule("/src/playback/presentation.ts");
after(async () => vite.close());

test("every playback phase presents Pause only for confirmed playing state", () => {
  assert.deepEqual(playPausePresentation(true), { icon: "pause", label: "Pause" });
  for (const phase of ["idle", "loading", "paused", "ended", "error"]) {
    const isPlaying = phase === "playing";
    assert.deepEqual(playPausePresentation(isPlaying), { icon: "play", label: "Play" });
  }
});

test("track rows, iPod, and bottom bar derive the icon from the shared presentation helper", async () => {
  const { readFile } = await import("node:fs/promises");
  const app = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.equal((app.match(/playPausePresentation\(/g) ?? []).length, 3);
  assert.match(app, /<Icon name=\{playPause\.icon\} size=\{17\} \/>/);
  assert.match(app, /<Icon name=\{playPause\.icon\} size=\{20\} \/>/);
});
