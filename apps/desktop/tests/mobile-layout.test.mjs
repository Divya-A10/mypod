import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

const app = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
const styles = await readFile(new URL("../src/styles.css", import.meta.url), "utf8");
const html = await readFile(new URL("../index.html", import.meta.url), "utf8");
const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

test("mobile search remains in navigation and on its page without a duplicate header control", () => {
  assert.doesNotMatch(app, /mobile-search-shortcut/);
  assert.match(app, /aria-label="Mobile navigation"[\s\S]*?onClick=\{\(\) => navigate\("Search"\)\}/);
  assert.match(app, /className="search-field mobile-search-field"[\s\S]*?aria-label="Search your library"/);
  assert.doesNotMatch(app, /className="search-field mobile-search-field"[^\n]*autoFocus/);
  assert.match(app, /event\.key === "Enter"\) event\.currentTarget\.blur\(\)/);
  assert.match(styles, /\.topbar-right \{ display: none; \}/);
});

test("mobile shell uses a bounded viewport and keeps scrolling in the content region", () => {
  assert.match(styles, /height: 100vh;\s*height: 100dvh;/);
  assert.match(styles, /\.main-scroll \{[\s\S]*?overflow-y: auto;[\s\S]*?overscroll-behavior-y: contain;/);
  assert.match(styles, /padding-top: env\(safe-area-inset-top\);/);
  assert.match(styles, /bottom: calc\(64px \+ env\(safe-area-inset-bottom\)\)/);
  assert.match(styles, /height: calc\(64px \+ env\(safe-area-inset-bottom\)\)/);
  assert.doesNotMatch(html, /user-scalable\s*=|maximum-scale\s*=/i);
});

test("mobile text controls meet the iOS focus-zoom font-size threshold and buttons do not scale on touch", () => {
  assert.match(styles, /\.main-panel input:not\(\[type="range"\]\):not\(\[type="file"\]\):not\(\[type="checkbox"\]\):not\(\[type="radio"\]\),[\s\S]*?font-size: 16px !important;/);
  assert.match(styles, /button:hover, button:active \{ transform: none !important; \}/);
  assert.match(styles, /\.wheel-prev, \.wheel-next \{ transform: none; \}/);
  assert.match(styles, /\.ipod-stage \{ animation: none; \}/);
});

test("the iPod player compacts for short iPhone viewports while keeping the wheel reachable", () => {
  assert.match(styles, /@media \(max-width: 760px\) and \(max-height: 720px\) \{[\s\S]*?\.player-scrim \{[\s\S]*?height: 100dvh;[\s\S]*?overflow-y: auto;[\s\S]*?safe-area-inset-bottom[\s\S]*?\}/);
  assert.match(styles, /@media \(max-width: 760px\) and \(max-height: 720px\)[\s\S]*?\.click-wheel \{ width: min\(218px, 62vw\); height: min\(218px, 62vw\);/);
  assert.match(styles, /\.ipod-stage \.click-wheel > \.wheel-play \{[^}]*bottom: 5%;/);
  assert.match(styles, /\.ipod-stage \.click-wheel > \.wheel-center \{[^}]*top: 50%; left: 50%;[^}]*transform: translate\(-50%, -50%\);/);
  assert.match(styles, /@media \(max-width: 760px\) and \(max-height: 600px\)[\s\S]*?\.ipod-stage \.click-wheel > \.wheel-play \{ left: calc\(50% - 21px\);/);
  assert.match(styles, /\.player-scrim \{[\s\S]*?height: 100dvh;[\s\S]*?overflow-y: auto;[\s\S]*?safe-area-inset-bottom/);
  assert.match(styles, /\.ipod-stage \.click-wheel > \.wheel-play \{[^}]*bottom: 5%;/);
});

test("elapsed playback remains visible while AVPlayer duration is temporarily unknown", () => {
  assert.match(appSource, /duration \? Math\.min\(playback\.currentTime, duration\) : Math\.max\(0, playback\.currentTime\)/);
});
