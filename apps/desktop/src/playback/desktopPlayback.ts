import { HtmlAudioPlaybackDriver } from "./HtmlAudioPlaybackDriver";
import { PlaybackController } from "./PlaybackController";

/** Desktop composition root: shared controller plus the HTML audio implementation. */
export const playbackController = new PlaybackController(new HtmlAudioPlaybackDriver());
