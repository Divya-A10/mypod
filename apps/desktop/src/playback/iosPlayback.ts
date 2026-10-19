import { PlaybackController } from "./PlaybackController";
import { IosNativePlaybackDriver } from "./IosNativePlaybackDriver";

/** iOS composition root: shared queue/history controller plus native AVPlayer. */
export const iosPlaybackController = new PlaybackController(new IosNativePlaybackDriver());
