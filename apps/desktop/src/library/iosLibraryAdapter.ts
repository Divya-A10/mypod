import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { BaseDirectory, copyFile, mkdir, remove, stat } from "@tauri-apps/plugin-fs";
import type { AudioImportResult, LibraryAdapter, Playlist, PlaylistEntry, SpotifyCsvPreview, SpotifyCsvPreviewResult, Track, TrackResponse } from "./contracts";

const MAX_IMPORT_BYTES = 1_073_741_824;
const MAX_SPOTIFY_CSV_BYTES = 10 * 1024 * 1024;
const MAX_SPOTIFY_CSV_RECORDS = 20_000;
const STAGING_DIR = "MyPod/Importing";
const SUPPORTED_EXTENSIONS = ["wav", "mp3", "m4a", "aac", "flac", "ogg", "aiff", "aif"];

type IosImportResponse = { track: Track; duplicate: boolean };
type IosLibraryAdapterDependencies = {
  invoke: typeof invoke;
  open: typeof open;
  copyFile: typeof copyFile;
  mkdir: typeof mkdir;
  remove: typeof remove;
  stat: typeof stat;
  randomId: () => string;
};

const defaultDependencies: IosLibraryAdapterDependencies = {
  invoke,
  open,
  copyFile,
  mkdir,
  remove,
  stat,
  randomId: () => {
    const bytes = crypto.getRandomValues(new Uint8Array(16));
    return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  },
};

function selectedFileParts(fileUrl: string): { originalName: string; extension: string } {
  const parsed = new URL(fileUrl);
  if (parsed.protocol !== "file:") throw new Error("The file picker returned an unsupported location.");
  const originalName = decodeURIComponent(parsed.pathname.split("/").pop() || "");
  const extension = originalName.split(".").pop()?.toLowerCase() || "";
  if (!originalName || !SUPPORTED_EXTENSIONS.includes(extension)) {
    throw new Error("Choose a supported audio file (WAV, MP3, M4A, AAC, FLAC, OGG, or AIFF).");
  }
  return { originalName, extension };
}

export function createIosLibraryAdapter(dependencies = defaultDependencies): LibraryAdapter {
  return {
    async listTracks(offset = 0) {
      return dependencies.invoke<TrackResponse>("list_tracks", { offset });
    },
    async getTrack(trackId) {
      if (!Number.isSafeInteger(trackId) || trackId < 1) throw new Error("A valid library track ID is required");
      return dependencies.invoke<Track>("get_track", { trackId });
    },
    async importAudio(): Promise<AudioImportResult> {
      const selected = await dependencies.open({
        multiple: false,
        pickerMode: "document",
        fileAccessMode: "scoped",
        filters: [{ name: "Audio", extensions: SUPPORTED_EXTENSIONS }],
      });
      if (selected === null) return { status: "cancelled" };
      if (Array.isArray(selected)) throw new Error("Select one audio file at a time.");

      const { originalName, extension } = selectedFileParts(selected);
      const info = await dependencies.stat(selected);
      if (!info.isFile || info.isSymlink || info.size <= 0 || info.size > MAX_IMPORT_BYTES) {
        throw new Error("The selected item is empty, not a regular file, or exceeds the 1 GiB import limit.");
      }

      const importId = dependencies.randomId();
      if (!/^[a-f0-9]{32}$/.test(importId)) throw new Error("Could not create a safe import token.");
      const stagedPath = `${STAGING_DIR}/${importId}.part`;
      await dependencies.mkdir(STAGING_DIR, { baseDir: BaseDirectory.AppLocalData, recursive: true });
      try {
        await dependencies.copyFile(selected, stagedPath, { toPathBaseDir: BaseDirectory.AppLocalData });
        const result = await dependencies.invoke<IosImportResponse>("register_imported_track", {
          importId,
          originalName,
          extension,
        });
        return { status: result.duplicate ? "duplicate" : "imported", track: result.track };
      } catch (error) {
        try {
          await dependencies.remove(stagedPath, { baseDir: BaseDirectory.AppLocalData });
        } catch {
          // The native catalog removes stale partials on its next list/import call.
        }
        throw error;
      }
    },
    async previewSpotifyCsv(): Promise<SpotifyCsvPreviewResult> {
      const selected = await dependencies.open({
        multiple: false,
        pickerMode: "document",
        fileAccessMode: "scoped",
        filters: [{ name: "Spotify CSV", extensions: ["csv"] }],
      });
      if (selected === null) return { status: "cancelled" };
      if (Array.isArray(selected)) throw new Error("Select one CSV file at a time.");

      const parsed = new URL(selected);
      const fileName = decodeURIComponent(parsed.pathname.split("/").pop() || "");
      if (parsed.protocol !== "file:" || !fileName || !fileName.toLowerCase().endsWith(".csv")) {
        throw new Error("Choose a CSV file from the iOS Files picker.");
      }
      const info = await dependencies.stat(selected);
      if (!info.isFile || info.isSymlink) throw new Error("The selected CSV must be a regular file.");
      if (info.size > MAX_SPOTIFY_CSV_BYTES) {
        throw new Error(`The selected CSV exceeds the ${MAX_SPOTIFY_CSV_BYTES / (1024 * 1024)} MiB preview limit.`);
      }

      const importId = dependencies.randomId();
      if (!/^[a-f0-9]{32}$/.test(importId)) throw new Error("Could not create a safe preview token.");
      const stagedPath = `${STAGING_DIR}/spotify-${importId}.csv.part`;
      await dependencies.mkdir(STAGING_DIR, { baseDir: BaseDirectory.AppLocalData, recursive: true });
      try {
        await dependencies.copyFile(selected, stagedPath, { toPathBaseDir: BaseDirectory.AppLocalData });
        const preview = await dependencies.invoke<SpotifyCsvPreview>("preview_spotify_csv", { importId });
        if (preview.summary.source_records > MAX_SPOTIFY_CSV_RECORDS) {
          throw new Error("The native preview returned an invalid record count.");
        }
        return { status: "previewed", preview };
      } finally {
        try {
          await dependencies.remove(stagedPath, { baseDir: BaseDirectory.AppLocalData });
        } catch {
          // Native command cleanup is authoritative; this is a best-effort fallback.
        }
      }
    },
    playlists: {
      listPlaylists: () => dependencies.invoke<Playlist[]>("list_playlists"),
      createPlaylist: (name) => dependencies.invoke<Playlist>("create_playlist", { name }),
      renamePlaylist: (playlistId, name) => dependencies.invoke<Playlist>("rename_playlist", { playlistId, name }),
      deletePlaylist: (playlistId) => dependencies.invoke<void>("delete_playlist", { playlistId }),
      listEntries: (playlistId) => dependencies.invoke<PlaylistEntry[]>("list_playlist_entries", { playlistId }),
      addTrack: (playlistId, trackId) => dependencies.invoke<PlaylistEntry>("add_track_to_playlist", { playlistId, trackId }),
      removeEntry: (playlistId, entryId) => dependencies.invoke<void>("remove_playlist_entry", { playlistId, entryId }),
      reorderEntries: (playlistId, entryIds) => dependencies.invoke<PlaylistEntry[]>("reorder_playlist_entries", { playlistId, entryIds }),
    },
  };
}

export const iosLibraryAdapter = createIosLibraryAdapter();
