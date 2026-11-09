import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

const app = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
const styles = await readFile(new URL("../src/styles.css", import.meta.url), "utf8");

test("all five click-wheel controls are rendered with stable accessible names", () => {
  for (const [name, label] of [
    ["wheel-menu", "Menu: close player"],
    ["wheel-prev", "Previous track"],
    ["wheel-next", "queue is empty"],
    ["wheel-play", "playPause.label"],
    ["wheel-center", "Select: find this track in your library"],
  ]) {
    assert.ok(app.includes(name), `missing ${name}`);
    assert.ok(app.includes(label), `missing accessible label text for ${name}`);
  }
  const wheel = app.slice(app.indexOf('<div className="click-wheel"'), app.indexOf("</div>", app.indexOf('<div className="click-wheel"')));
  assert.ok(wheel.indexOf("wheel-prev") < wheel.indexOf("wheel-next"));
  assert.ok(wheel.indexOf("wheel-next") < wheel.indexOf("wheel-play"));
  assert.match(wheel, /wheel-prev[^]*name="rewind"/);
  assert.match(wheel, /wheel-next[^]*name="fast-forward"/);
});

test("wheel transport geometry is explicitly fixed and remains fixed while pressed", () => {
  for (const rule of [
    /\.ipod-stage \.click-wheel > \.wheel-prev\s*\{[^}]*top:\s*calc\(50% - 24px\)[^}]*left:\s*5%[^}]*transform:\s*none/s,
    /\.ipod-stage \.click-wheel > \.wheel-next\s*\{[^}]*top:\s*calc\(50% - 24px\)[^}]*right:\s*5%[^}]*transform:\s*none/s,
    /\.ipod-stage \.click-wheel > \.wheel-play\s*\{[^}]*bottom:\s*5%[^}]*left:\s*calc\(50% - 24px\)[^}]*transform:\s*none/s,
    /\.ipod-stage \.click-wheel > \.wheel-center\s*\{[^}]*top:\s*50%[^}]*left:\s*50%[^}]*width:\s*88px[^}]*height:\s*88px[^}]*transform:\s*translate\(-50%, -50%\)/s,
    /\.ipod-stage \.click-wheel > \.wheel-menu:hover,\s*\.ipod-stage \.click-wheel > \.wheel-menu:active\s*\{\s*transform:\s*translateX\(-50%\)/s,
  ]) assert.match(styles, rule);
});

test("Home album cards provide their real library track ID to the artwork loader", () => {
  assert.match(app, /albums\.slice\(0, 4\)\.map\(\(album\) => <CollectionCard[^\n]*artworkTrackId=\{album\.id\}[^\n]*openAlbum\(album\)/);
  assert.match(app, /<Art label=\{activeAlbum\.album[^\n]*trackId=\{activeAlbum\.id\}/);
  assert.match(app, /<Art label=\{track\.album[^\n]*trackId=\{track\.id\}/);
  assert.match(app, /trackId=\{displayTrack\?\.id\}/);
});
