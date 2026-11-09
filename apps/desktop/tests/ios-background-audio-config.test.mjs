import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

const sourceRoot = new URL("../src-tauri/", import.meta.url);
const [tauriConfig, projectTemplate, generatedProject, generatedInfo, playback] = await Promise.all([
  readFile(new URL("tauri.conf.json", sourceRoot), "utf8"),
  readFile(new URL("ios-project.yml", sourceRoot), "utf8"),
  readFile(new URL("gen/apple/project.yml", sourceRoot), "utf8"),
  readFile(new URL("gen/apple/mypod-desktop_iOS/Info.plist", sourceRoot), "utf8"),
  readFile(new URL("src/ios_playback.rs", sourceRoot), "utf8"),
]);

test("iOS release configuration preserves audio background mode through Tauri regeneration", () => {
  const config = JSON.parse(tauriConfig);
  assert.equal(config.identifier, "app.mypod.local");
  assert.equal(config.bundle.iOS.template, "ios-project.yml");
  assert.match(projectTemplate, /com\.apple\.BackgroundModes:[\s\S]*?enabled: 1/);
  assert.match(projectTemplate, /UIBackgroundModes: \[audio\]/);
  assert.match(generatedProject, /com\.apple\.BackgroundModes:[\s\S]*?enabled: 1/);
  assert.match(generatedProject, /UIBackgroundModes: \[audio\]/);
  assert.match(generatedInfo, /<key>UIBackgroundModes<\/key>[\s\S]*?<string>audio<\/string>/);
});

test("native AVPlayer owns periodic progress and removes the observer on track replacement", () => {
  assert.match(playback, /addPeriodicTimeObserverForInterval_queue_usingBlock/);
  assert.match(playback, /removeTimeObserver/);
  assert.match(playback, /app\.emit\("local-playback-progress"/);
  assert.match(playback, /AVAudioSessionCategoryPlayback/);
  assert.match(playback, /install_remote_commands/);
});
