import { isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { getTrack, getTracks, previewSpotifyCsv, scanAudioFolder } from "../api";
import type { LibraryAdapter, SpotifyCsvPreviewResult } from "./contracts";

type DesktopAdapterDependencies = {
  isTauri: () => boolean;
  openDirectory: () => Promise<string | string[] | null>;
  listTracks: typeof getTracks;
  getTrack: typeof getTrack;
  previewSpotifyCsv: typeof previewSpotifyCsv;
  scanAudioFolder: typeof scanAudioFolder;
};

const defaultDependencies: DesktopAdapterDependencies = {
  isTauri,
  openDirectory: async () => await open({ directory: true, multiple: false }),
  listTracks: getTracks,
  getTrack,
  previewSpotifyCsv,
  scanAudioFolder,
};

/** FastAPI-backed adapter. Native folder scanning is exposed only in Tauri. */
export function createDesktopLibraryAdapter(dependencies = defaultDependencies): LibraryAdapter {
  return {
    listTracks: dependencies.listTracks,
    getTrack: dependencies.getTrack,
    async previewSpotifyCsv(file): Promise<SpotifyCsvPreviewResult> {
      if (!file) throw new Error("Choose a CSV file to preview.");
      return { status: "previewed", preview: await dependencies.previewSpotifyCsv(file) };
    },
    ...(dependencies.isTauri() ? {
      async scanAudioFolder() {
        const selected = await dependencies.openDirectory();
        if (selected === null) return { status: "cancelled" as const };
        if (Array.isArray(selected)) throw new Error("Select one permitted music folder at a time.");
        return { status: "scanned" as const, result: await dependencies.scanAudioFolder(selected) };
      },
    } : {}),
  };
}

export const desktopLibraryAdapter = createDesktopLibraryAdapter();
