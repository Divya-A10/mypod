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

test("iOS driver exposes ready and failed AVPlayer state without logging track metadata or paths", async () => {
  const infos = [];
  const originalInfo = console.info;
  console.info = (...values) => infos.push(values);
  try {
    const { driver, poll } = setup([
      nativeState({ ready: true, player_status: "ready", item_status: "ready", duration: 16 }),
      nativeState({ error: "native failure", failure_code: -11828, player_status: "failed", item_status: "failed" }),
    ]);
    const events = [];
    driver.subscribe((event) => events.push(event));
    driver.load(12);
    await driver.play();
    await new Promise((resolve) => setImmediate(resolve));
    await poll();
    assert.equal(events.some(({ type }) => type === "playing"), false);
    assert.equal(events.some(({ type, message }) => type === "error" && message.includes("-11828")), true);
    assert.equal(events.some(({ type, duration }) => type === "metadata" && duration === 16), true);
    assert.equal(infos.some(([, state]) => state?.ready === true), true);
    assert.doesNotMatch(JSON.stringify(infos), /private|Artist|Title|\.mp3|\/Users\//);
  } finally {
    console.info = originalInfo;
  }
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

test("iOS seek delegates to the native playback command without changing audio source", async () => {
  const { driver, calls, poll } = setup([
    nativeState({ playing: true, current_time: 3 }),
    nativeState({ playing: true, current_time: 8 }),
    nativeState({ playing: true, current_time: 8 }),
  ]);
  driver.load(12);
  await driver.play();
  driver.seek(8);
  await new Promise((resolve) => setImmediate(resolve));
  await poll();
  assert.deepEqual(calls.find(({ command }) => command === "seek_local_playback"), {
    command: "seek_local_playback",
    args: { seconds: 8 },
  });
  assert.equal(calls.some(({ args }) => args && Object.values(args).some((value) => typeof value === "string" && value.includes("/"))), false);
});

test("native AVPlayer progress events update elapsed time and duration without a JavaScript counter", async () => {
  let progressHandler;
  let tick;
  const calls = [];
  const driver = new IosNativePlaybackDriver({
    invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === "local_playback_state") return nativeState({ playing: true, current_time: 0, duration: 125 });
      return undefined;
    },
    listen: async (name, handler) => { assert.equal(name, "local-playback-progress"); progressHandler = handler; return () => {}; },
    setInterval: (callback) => { tick = callback; return 1; },
    clearInterval: () => { tick = undefined; },
  });
  const events = [];
  driver.subscribe((event) => events.push(event));
  driver.load(12);
  await driver.play();
  progressHandler({ payload: { track_id: 12, current_time: 7.25, duration: 125 } });
  assert.equal(driver.currentTime, 7.25);
  assert.equal(driver.duration, 125);
  assert.equal(events.some((event) => event.type === "metadata" && event.duration === 125), true);
  assert.equal(events.some((event) => event.type === "timeupdate" && event.currentTime === 7.25), true);

  // Advancing wall-clock time does not advance the displayed timer by itself.
  await new Promise((resolve) => setTimeout(resolve, 15));
  assert.equal(driver.currentTime, 7.25);
  assert.equal(calls.some(({ command }) => command === "local_playback_state"), true);
  tick?.();
});

test("native pause, resume, seek, and track changes are reflected from AVPlayer state", async () => {
  const calls = [];
  let progressHandler;
  let state = nativeState({ playing: true, current_time: 4, duration: 90 });
  const secondTrack = { id: 13, title: "Second synthetic", artist: null, album: null, duration: 60, format: "wav", file_size: 1, modified_at: 1, created_at: "", updated_at: "", is_missing: 0 };
  const driver = new IosNativePlaybackDriver({
    invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === "local_playback_state") return state;
      if (command === "get_track") return secondTrack;
      if (command === "seek_local_playback") return undefined;
      if (command === "pause_local_playback") return undefined;
      if (command === "play_local_track") return undefined;
      return undefined;
    },
    listen: async (_name, handler) => { progressHandler = handler; return () => {}; },
    setInterval: () => 1,
    clearInterval: () => {},
  });
  const events = [];
  driver.subscribe((event) => events.push(event));
  driver.load(12);
  await driver.play();
  progressHandler({ payload: { track_id: 12, current_time: 4, duration: 90 } });
  driver.seek(21);
  await new Promise((resolve) => setImmediate(resolve));
  state = nativeState({ playing: false, current_time: 21, duration: 90 });
  progressHandler({ payload: { track_id: 12, current_time: 21, duration: 90 } });
  assert.equal(driver.currentTime, 21);
  assert.equal(events.some((event) => event.type === "timeupdate" && event.currentTime === 21), true);
  driver.pause();
  await new Promise((resolve) => setImmediate(resolve));
  await driver["poll"]();
  assert.equal(events.some((event) => event.type === "pause"), true);
  state = nativeState({ playing: true, current_time: 21, duration: 90 });
  await driver.play();
  await driver["poll"]();
  assert.equal(events.some((event) => event.type === "playing"), true);

  state = nativeState({ track_id: 13, playing: true, current_time: 0, duration: 60 });
  await driver["adoptNativeTrack"](state);
  assert.equal(driver.currentTime, 0);
  assert.equal(driver.duration, 60);
  assert.equal(events.some((event) => event.type === "externaltrack" && event.track?.id === 13), true);
  assert.equal(calls.some(({ command, args }) => command === "seek_local_playback" && args.seconds === 21), true);
});
