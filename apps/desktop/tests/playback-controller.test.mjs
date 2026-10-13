import assert from "node:assert/strict";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

class FakeAudio extends EventTarget {
  paused = true;
  duration = 30;
  error = null;
  preload = "";
  src = "";
  dispatchPlaying = true;
  rejectNextPlay = null;
  _currentTime = 0;

  get currentTime() {
    return this._currentTime;
  }

  set currentTime(value) {
    this._currentTime = value;
    this.dispatchEvent(new Event("seeked"));
  }

  load() {
    this._currentTime = 0;
    this.dispatchEvent(new Event("loadstart"));
    this.dispatchEvent(new Event("loadedmetadata"));
  }

  async play() {
    if (this.rejectNextPlay) {
      const error = this.rejectNextPlay;
      this.rejectNextPlay = null;
      throw error;
    }
    this.paused = false;
    if (this.dispatchPlaying) this.dispatchEvent(new Event("playing"));
  }

  pause() {
    const wasPlaying = !this.paused;
    this.paused = true;
    if (wasPlaying) this.dispatchEvent(new Event("pause"));
  }

  finish() {
    this.paused = true;
    this._currentTime = this.duration;
    this.dispatchEvent(new Event("ended"));
  }

  fail(code) {
    this.paused = true;
    this.error = { code };
    this.dispatchEvent(new Event("error"));
  }
}

globalThis.Audio = FakeAudio;
globalThis.MediaError = { MEDIA_ERR_SRC_NOT_SUPPORTED: 4 };

const vite = await createServer({
  configFile: fileURLToPath(new URL("../vite.config.ts", import.meta.url)),
  appType: "custom",
  logLevel: "silent",
  server: { middlewareMode: true },
});
const { PlaybackController, PlaybackQueue } = await vite.ssrLoadModule("/src/playback/PlaybackController.ts");
after(async () => vite.close());

function track(id, title = `Synthetic ${id}`) {
  return {
    id,
    path: `/synthetic/${id}.wav`,
    storage_type: "external",
    title,
    artist: "Test artist",
    album: "Test album",
    duration: 30,
    format: "wav",
    file_size: 64,
    modified_at: 0,
    created_at: "synthetic",
    updated_at: "synthetic",
    is_missing: 0,
  };
}

function setup() {
  const media = new FakeAudio();
  const controller = new PlaybackController(media, new PlaybackQueue());
  return { media, controller };
}

test("queue preserves library IDs, order, and repeated entries without starting playback", () => {
  const { controller } = setup();
  controller.enqueue(track(7));
  controller.enqueue(track(3));
  controller.enqueue(track(7, "Repeated synthetic track"));

  assert.deepEqual(controller.getSnapshot().queue.map(({ trackId }) => trackId), [7, 3, 7]);
  assert.equal(controller.getSnapshot().track, null);
  assert.equal(controller.getSnapshot().phase, "idle");
});

test("next keeps its first entry queued until a real playing event", async () => {
  const { media, controller } = setup();
  controller.enqueue(track(11));
  media.dispatchPlaying = false;

  await controller.next();
  assert.equal(controller.getSnapshot().phase, "loading");
  assert.deepEqual(controller.getSnapshot().queue.map(({ trackId }) => trackId), [11]);
  assert.deepEqual(controller.getSnapshot().history, []);

  media.dispatchEvent(new Event("playing"));
  assert.equal(controller.getSnapshot().phase, "playing");
  assert.deepEqual(controller.getSnapshot().queue, []);
});

test("next consumes entries in FIFO order, one successful start at a time", async () => {
  const { controller } = setup();
  controller.enqueue(track(12));
  controller.enqueue(track(13));

  await controller.next();
  assert.equal(controller.getSnapshot().track.id, 12);
  assert.deepEqual(controller.getSnapshot().queue.map(({ trackId }) => trackId), [13]);

  await controller.next();
  assert.equal(controller.getSnapshot().track.id, 13);
  assert.deepEqual(controller.getSnapshot().queue, []);
});

test("rejected or failed queued playback remains available for retry", async () => {
  const { media, controller } = setup();
  controller.enqueue(track(21));
  media.rejectNextPlay = new DOMException("Blocked", "NotAllowedError");

  await controller.next();
  assert.equal(controller.getSnapshot().phase, "error");
  assert.match(controller.getSnapshot().error, /blocked/i);
  assert.deepEqual(controller.getSnapshot().queue.map(({ trackId }) => trackId), [21]);

  await controller.next();
  assert.equal(controller.getSnapshot().phase, "playing");
  assert.deepEqual(controller.getSnapshot().queue, []);
});

test("a media error leaves the queued item pending and retryable", async () => {
  const { media, controller } = setup();
  controller.enqueue(track(25));
  media.dispatchPlaying = false;

  await controller.next();
  media.fail(4);
  assert.equal(controller.getSnapshot().phase, "error");
  assert.deepEqual(controller.getSnapshot().queue.map(({ trackId }) => trackId), [25]);
  assert.deepEqual(controller.getSnapshot().history, []);

  media.dispatchPlaying = true;
  await controller.next();
  assert.equal(controller.getSnapshot().phase, "playing");
  assert.deepEqual(controller.getSnapshot().queue, []);
});

test("direct-track history is committed only after a retry actually starts", async () => {
  const { media, controller } = setup();
  media.dispatchPlaying = false;
  await controller.play(track(26));
  media.dispatchEvent(new Event("playing"));

  media.rejectNextPlay = new DOMException("Unavailable", "NotSupportedError");
  await controller.play(track(27));
  assert.equal(controller.getSnapshot().phase, "error");
  assert.deepEqual(controller.getSnapshot().history, []);

  media.dispatchPlaying = true;
  await controller.play(track(27));
  assert.equal(controller.getSnapshot().phase, "playing");
  assert.deepEqual(controller.getSnapshot().history.map(({ trackId }) => trackId), [26]);
});

test("ended advances to the next item and records only tracks that actually started", async () => {
  const { media, controller } = setup();
  media.dispatchPlaying = false;
  await controller.play(track(31));
  assert.deepEqual(controller.getSnapshot().history, []);

  media.dispatchEvent(new Event("playing"));
  assert.equal(controller.getSnapshot().phase, "playing");
  controller.enqueue(track(32));
  media.dispatchPlaying = true;
  media.finish();

  assert.equal(controller.getSnapshot().track.id, 32);
  assert.equal(controller.getSnapshot().phase, "playing");
  assert.deepEqual(controller.getSnapshot().history.map(({ trackId }) => trackId), [31]);
  assert.deepEqual(controller.getSnapshot().queue, []);
});

test("previous restarts after three seconds and otherwise returns to successful history", async () => {
  const { media, controller } = setup();
  await controller.play(track(41));
  await controller.play(track(42));
  assert.deepEqual(controller.getSnapshot().history.map(({ trackId }) => trackId), [41]);

  media.currentTime = 8;
  await controller.previous();
  assert.equal(controller.getSnapshot().track.id, 42);
  assert.equal(controller.getSnapshot().currentTime, 0);
  assert.deepEqual(controller.getSnapshot().history.map(({ trackId }) => trackId), [41]);

  media.currentTime = 1;
  await controller.previous();
  assert.equal(controller.getSnapshot().track.id, 41);
  assert.deepEqual(controller.getSnapshot().history, []);
  assert.deepEqual(controller.getSnapshot().queue.map(({ trackId }) => trackId), [42]);
});

test("previous without history restarts the current track and empty next is a no-op", async () => {
  const { media, controller } = setup();
  await controller.play(track(51));
  media.currentTime = 2;
  await controller.previous();
  assert.equal(controller.getSnapshot().track.id, 51);
  assert.equal(controller.getSnapshot().currentTime, 0);

  const snapshot = controller.getSnapshot();
  await controller.next();
  assert.equal(controller.getSnapshot().track.id, snapshot.track.id);
  assert.deepEqual(controller.getSnapshot().queue, []);
});

test("queue entries can be removed individually or cleared", () => {
  const { controller } = setup();
  const first = controller.enqueue(track(61));
  controller.enqueue(track(62));
  controller.removeFromQueue(first.entryId);
  assert.deepEqual(controller.getSnapshot().queue.map(({ trackId }) => trackId), [62]);
  controller.clearQueue();
  assert.deepEqual(controller.getSnapshot().queue, []);
});
