import { isIOSPlatform, type NavigatorPlatformInfo } from "../platform";
import { desktopLibraryAdapter } from "./desktopLibraryAdapter";
import { iosLibraryAdapter } from "./iosLibraryAdapter";
import type { LibraryAdapter } from "./contracts";

/** Selects an explicit platform adapter; iOS never falls back to the desktop API. */
export function createLibraryAdapter(info?: NavigatorPlatformInfo): LibraryAdapter {
  const platform = info ?? (typeof navigator === "undefined" ? null : navigator);
  if (platform && isIOSPlatform(platform)) return iosLibraryAdapter;
  return desktopLibraryAdapter;
}

export const libraryAdapter = createLibraryAdapter();
