import { getTrack, getTracks } from "../api";
import type { LibraryAdapter } from "./contracts";

/** Existing FastAPI-backed library implementation for desktop only. */
export const desktopLibraryAdapter: LibraryAdapter = {
  listTracks: getTracks,
  getTrack,
};
