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
const { IosNativePlaybackDriver } = await vite.ssrLoadModule("/src/playback/IosNativePlaybackDriver.ts");
after(async () => vite.close());

function setup(states) {
  const calls = [];
  let tick;
  const driver = new IosNativePlaybackDriver({
    invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === "local_playback_state") return states.shift();
      return undefined;
    },
    setInterval: (callback) => { tick = callback; return 1; },
    clearInterval: () => { tick = undefined; },
  });
  return { driver, calls, poll: async () => { tick?.(); await new Promise((resolve) => setImmediate(resolve)); } };
}

const nativeState = (patch = {}) => ({
  track_id: 12,
  current_time: 0,
  duration: 12,
  playing: false,
  loading: false,
  ended: false,
  error: null,
  ...patch,
});

test("iOS driver reports playing only after native state confirms playback and never receives a path", async () => {
  const { driver, calls, poll } = setup([
    nativeState({ loading: true }),
    nativeState({ playing: true, current_time: 1 }),
    nativeState({ playing: true, current_time: 2 }),
    nativeState({ playing: false, ended: true, current_time: 12 }),
  ]);
  const events = [];
  driver.subscribe((event) => events.push(event));
  driver.load(12);
  assert.deepEqual(events.map(({ type }) => type), ["loadstart"]);

  await driver.play();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(events.some(({ type }) => type === "playing"), false);
  await poll();
  assert.equal(events.some(({ type }) => type === "playing"), true);
  await poll();
  assert.equal(events.some(({ type }) => type === "ended"), false);
  await poll();
  assert.equal(events.some(({ type }) => type === "ended"), true);
  assert.deepEqual(calls[0], { command: "play_local_track", args: { trackId: 12, reload: true } });
  assert.equal(calls.some(({ args }) => args && Object.values(args).some((value) => typeof value === "string" && value.includes("/"))), false);
});

test("iOS driver preserves reload intent after failure and reuses the player for resume", async () => {
  const calls = [];
  let rejectFirst = true;
  const driver = new IosNativePlaybackDriver({
    invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === "play_local_track" && rejectFirst) {
        rejectFirst = false;
        throw new Error("native failure");
      }
      return nativeState({ playing: true });
    },
    setInterval: () => 1,
    clearInterval: () => {},
  });

  driver.load(12);
  await assert.rejects(driver.play(), /could not start this track/);
  await driver.play();
  await driver.play();

  assert.deepEqual(calls.filter(({ command }) => command === "play_local_track").map(({ args }) => args), [
    { trackId: 12, reload: true },
    { trackId: 12, reload: true },
    { trackId: 12, reload: false },
  ]);
});

test("iOS driver reports native command failures safely and does not overstate media errors", async () => {
  const warnings = [];
  const originalWarn = console.warn;
  console.warn = (...values) => warnings.push(values.join(" "));
  try {
    const failedCommand = new IosNativePlaybackDriver({
      invoke: async (command) => { if (command === "play_local_track") throw new Error("/private/path/Artist - Title.wav"); return nativeState(); },
      setInterval,
      clearInterval,
    });
    failedCommand.load(12);
    await assert.rejects(failedCommand.play(), /could not start this track/);
  } finally {
    console.warn = originalWarn;
  }
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /native play command failed/);
  assert.doesNotMatch(warnings[0], /private|Artist|Title|wav/);

  const { driver, poll } = setup([nativeState({ error: "unsupported codec" })]);
  const events = [];
  driver.subscribe((event) => events.push(event));
  driver.load(12);
  await driver.play();
  await poll();
  assert.equal(events.some(({ type, unsupported }) => type === "error" && !unsupported), true);
});

test("iOS driver reports rejected pause and seek commands as media errors", async () => {
  for (const [command, action] of [
    ["pause_local_playback", (driver) => driver.pause()],
    ["seek_local_playback", (driver) => driver.seek(2)],
  ]) {
    const driver = new IosNativePlaybackDriver({
      invoke: async (name) => {
        if (name === command) throw new Error(`${name} rejected`);
        return nativeState({ playing: true });
      },
      setInterval,
      clearInterval,
    });
    const events = [];
    driver.subscribe((event) => events.push(event));
    driver.load(12);
    action(driver);
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(events.some(({ type }) => type === "error"), true, `${command} should emit an error`);
    assert.equal(driver.paused, true, `${command} rejection must not claim active playback`);
  }
});
