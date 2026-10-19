export type NavigatorPlatformInfo = Pick<Navigator, "userAgent" | "platform" | "maxTouchPoints">;

/** Detect iPhone/iPad/iPod WebViews, including iPadOS desktop-style user agents. */
export function isIOSPlatform(info: NavigatorPlatformInfo): boolean {
  return /iPhone|iPad|iPod/i.test(info.userAgent)
    || (info.platform === "MacIntel" && info.maxTouchPoints > 1);
}

/** Desktop-only adapters must fail explicitly instead of targeting localhost on iOS. */
export function assertDesktopAdapterAvailable(info?: NavigatorPlatformInfo): void {
  const currentInfo = info ?? (typeof navigator === "undefined" ? null : navigator);
  if (currentInfo && isIOSPlatform(currentInfo)) {
    throw new Error("The iOS library and playback adapters are not implemented yet.");
  }
}
