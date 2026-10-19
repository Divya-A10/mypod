import { isTauri } from "@tauri-apps/api/core";
import { isIOSPlatform } from "../platform";
import { playbackController as desktopPlaybackController } from "./desktopPlayback";
import { iosPlaybackController } from "./iosPlayback";

const currentPlatform = typeof navigator === "undefined" ? null : navigator;
const isIOS = currentPlatform ? isIOSPlatform(currentPlatform) : false;

/** Selects an explicit native iOS or HTML media implementation. */
export const playbackController = isIOS ? iosPlaybackController : desktopPlaybackController;
export const playbackAvailable = !isIOS || isTauri();
