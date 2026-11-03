import { invoke } from "@tauri-apps/api/core";
import { open, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { BaseDirectory, copyFile, mkdir, remove, stat, startAccessingSecurityScopedResource, stopAccessingSecurityScopedResource } from "@tauri-apps/plugin-fs";
import type { AudioImportResult, BackupAdapter, BackupSummary, LibraryAdapter, Playlist, PlaylistEntry, SpotifyCsvPreview, SpotifyCsvPreviewResult, Track, TrackResponse } from "./contracts";

const MAX_IMPORT_BYTES = 1_073_741_824;
const MAX_SPOTIFY_CSV_BYTES = 10 * 1024 * 1024;
const MAX_SPOTIFY_CSV_RECORDS = 20_000;
const STAGING_DIR = "MyPod/Importing";
const BACKUP_DIR = "MyPod/Backups";
const SUPPORTED_EXTENSIONS = ["wav", "mp3", "m4a", "aac", "flac", "ogg", "aiff", "aif"];
const CSV_PREVIEW_BUILD = "csv-preview-copy-2026-10-19-r2";
const MAX_ARTWORK_BYTES = 8 * 1024 * 1024;

function artworkMimeType(bytes: Uint8Array): string | null {
  if (bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) return "image/jpeg";
  if (bytes.length >= 8 && [137, 80, 78, 71, 13, 10, 26, 10].every((byte, index) => bytes[index] === byte)) return "image/png";
  if (bytes.length >= 6 && (new TextDecoder().decode(bytes.subarray(0, 6)) === "GIF87a" || new TextDecoder().decode(bytes.subarray(0, 6)) === "GIF89a")) return "image/gif";
  if (bytes.length >= 12 && new TextDecoder().decode(bytes.subarray(0, 4)) === "RIFF" && new TextDecoder().decode(bytes.subarray(8, 12)) === "WEBP") return "image/webp";
  return null;
}

type IosImportResponse = { track: Track; duplicate: boolean; metadata_warning?: string | null };
type IosLibraryAdapterDependencies = {
  invoke: typeof invoke;
  open: typeof open;
  save: typeof saveDialog;
  copyFile: typeof copyFile;
  mkdir: typeof mkdir;
  remove: typeof remove;
  stat: typeof stat;
  startAccessingSecurityScopedResource: typeof startAccessingSecurityScopedResource;
  stopAccessingSecurityScopedResource: typeof stopAccessingSecurityScopedResource;
  randomId: () => string;
};

const defaultDependencies: IosLibraryAdapterDependencies = {
  invoke,
  open,
  save: saveDialog,
  copyFile,
  mkdir,
  remove,
  stat,
  startAccessingSecurityScopedResource,
  stopAccessingSecurityScopedResource,
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
  const backups: BackupAdapter = {
    async createBackup(scope) {
      const destination = await dependencies.save({
        defaultPath: "MyPod-backup.mypodbackup",
        filters: [{ name: "MyPod backup", extensions: ["mypodbackup"] }],
      });
      if (destination === null) return { status: "cancelled" };
      const token = dependencies.randomId();
      if (!/^[a-f0-9]{32}$/.test(token)) throw new Error("Could not create a safe backup token.");
      await dependencies.mkdir(BACKUP_DIR, { baseDir: BaseDirectory.AppLocalData, recursive: true });
      const summary = await dependencies.invoke<BackupSummary>("create_local_backup", { token, scope });
      try {
        await dependencies.copyFile(`${BACKUP_DIR}/${token}.mypodbackup`, destination, { fromPathBaseDir: BaseDirectory.AppLocalData });
        await dependencies.remove(`${BACKUP_DIR}/${token}.mypodbackup`, { baseDir: BaseDirectory.AppLocalData });
        return { status: "created", token, summary };
      } catch (error) {
        // Keep the completed app-private bundle if export to the selected Files
        // destination fails (including a full destination volume).
        return { status: "created", token, summary, exportError: error instanceof Error ? error.message : String(error) };
      }
    },
    async exportBackup(token) {
      if (!/^[a-f0-9]{32}$/.test(token)) throw new Error("Invalid backup token.");
      const destination = await dependencies.save({
        defaultPath: "MyPod-backup.mypodbackup",
        filters: [{ name: "MyPod backup", extensions: ["mypodbackup"] }],
      });
      if (destination === null) return false;
      const source = `${BACKUP_DIR}/${token}.mypodbackup`;
      await dependencies.copyFile(source, destination, { fromPathBaseDir: BaseDirectory.AppLocalData });
      await dependencies.remove(source, { baseDir: BaseDirectory.AppLocalData });
      return true;
    },
    async inspectBackup() {
      const selected = await dependencies.open({
        multiple: false,
        pickerMode: "document",
        fileAccessMode: "copy",
        filters: [{ name: "MyPod backup", extensions: ["mypodbackup"] }],
      });
      if (selected === null) return { status: "cancelled" };
      if (Array.isArray(selected)) throw new Error("Select one MyPod backup at a time.");
      const parsed = new URL(selected);
      if (parsed.protocol !== "file:") throw new Error("Choose a MyPod backup from Files.");
      const info = await dependencies.stat(selected);
      if (!info.isFile || info.isSymlink || info.size <= 0) throw new Error("The selected backup is not a regular file.");
      const token = dependencies.randomId();
      if (!/^[a-f0-9]{32}$/.test(token)) throw new Error("Could not create a safe restore token.");
      await dependencies.mkdir(STAGING_DIR, { baseDir: BaseDirectory.AppLocalData, recursive: true });
      const stagedPath = `${STAGING_DIR}/backup-${token}.part`;
      await dependencies.copyFile(selected, stagedPath, { toPathBaseDir: BaseDirectory.AppLocalData });
      const summary = await dependencies.invoke<BackupSummary>("inspect_local_backup", { token });
      return { status: "ready", token, summary };
    },
    restoreBackup: (token) => dependencies.invoke<BackupSummary>("restore_local_backup", { token, confirmed: true }),
  };

  return {
    backups,
    async listTracks(offset = 0) {
      return dependencies.invoke<TrackResponse>("list_tracks", { offset });
    },
    async getTrack(trackId) {
      if (!Number.isSafeInteger(trackId) || trackId < 1) throw new Error("A valid library track ID is required");
      return dependencies.invoke<Track>("get_track", { trackId });
    },
    async getTrackArtwork(trackId) {
      if (!Number.isSafeInteger(trackId) || trackId < 1) throw new Error("A valid library track ID is required");
      const payload = await dependencies.invoke<ArrayBuffer>("get_track_artwork", { trackId });
      if (!(payload instanceof ArrayBuffer) || payload.byteLength > MAX_ARTWORK_BYTES) throw new Error("The on-device catalog returned invalid artwork.");
      if (payload.byteLength === 0) return null;
      const bytes = new Uint8Array(payload);
      const mimeType = artworkMimeType(bytes);
      if (!mimeType) throw new Error("The on-device catalog returned unsupported artwork.");
      return new Blob([bytes], { type: mimeType });
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
      let info;
      try {
        info = await dependencies.stat(selected);
      } catch (error) {
        throw new Error(`CSV file access failed: ${error instanceof Error ? error.message : String(error)}`);
      }
      if (!info.isFile || info.isSymlink || info.size <= 0 || info.size > MAX_IMPORT_BYTES) {
        throw new Error("The selected item is empty, not a regular file, or exceeds the 1 GiB import limit.");
      }

      const importId = dependencies.randomId();
      if (!/^[a-f0-9]{32}$/.test(importId)) throw new Error("Could not create a safe import token.");
      const stagedPath = `${STAGING_DIR}/${importId}.part`;
      await dependencies.mkdir(STAGING_DIR, { baseDir: BaseDirectory.AppLocalData, recursive: true });
      try {
        try {
          await dependencies.copyFile(selected, stagedPath, { toPathBaseDir: BaseDirectory.AppLocalData });
        } catch (error) {
          throw new Error(`CSV file access failed: ${error instanceof Error ? error.message : String(error)}`);
        }
        const result = await dependencies.invoke<IosImportResponse>("register_imported_track", {
          importId,
          originalName,
          extension,
        });
        return { status: result.duplicate ? "duplicate" : "imported", track: result.track, ...(result.metadata_warning ? { metadataWarning: result.metadata_warning } : {}) };
      } catch (error) {
        // Keep app-private partial copies for later recovery/review. Import
        // failures are not proof that the staged media is safe to discard.
        throw error;
      }
    },
    async previewSpotifyCsv(): Promise<SpotifyCsvPreviewResult> {
      const selected = await dependencies.open({
        multiple: false,
        pickerMode: "document",
        // Materialize the Files-provider document in the app sandbox so the
        // parser receives a stable, app-owned file.
        fileAccessMode: "copy",
        // iOS Files providers may advertise Safari-saved CSVs as generic data
        // instead of the CSV UTI, which makes an extension-filtered item
        // appear disabled. Validation below still gates every selected file.
      });
      if (selected === null) return { status: "cancelled" };
      if (Array.isArray(selected)) throw new Error("Select one CSV file at a time.");
      console.info("[MyPod CSV] selection completed", { build: CSV_PREVIEW_BUILD, protocol: new URL(selected).protocol });


      const parsed = new URL(selected);
      const fileName = decodeURIComponent(parsed.pathname.split("/").pop() || "");
      if (parsed.protocol !== "file:" || !fileName || !fileName.toLowerCase().endsWith(".csv")) {
        throw new Error("Choose a CSV file from the iOS Files picker.");
      }
      let info;
      try {
        info = await dependencies.stat(selected);
        console.info("[MyPod CSV] sandbox file access succeeded", { build: CSV_PREVIEW_BUILD, bytes: info.size, regular: info.isFile, symlink: info.isSymlink });
      } catch (error) {
        console.error("[MyPod CSV] sandbox file access failed", { build: CSV_PREVIEW_BUILD, error: String(error) });
        throw new Error(`CSV file access failed: ${error instanceof Error ? error.message : String(error)}`);
      }
      if (!info.isFile || info.isSymlink) throw new Error("The selected CSV must be a regular file.");
      if (info.size > MAX_SPOTIFY_CSV_BYTES) {
        throw new Error(`The selected CSV exceeds the ${MAX_SPOTIFY_CSV_BYTES / (1024 * 1024)} MiB preview limit.`);
      }

      const importId = dependencies.randomId();
      if (!/^[a-f0-9]{32}$/.test(importId)) throw new Error("Could not create a safe preview token.");
      const stagedPath = `${STAGING_DIR}/spotify-${importId}.csv.part`;
      await dependencies.mkdir(STAGING_DIR, { baseDir: BaseDirectory.AppLocalData, recursive: true });
      try {
        try {
          await dependencies.copyFile(selected, stagedPath, { toPathBaseDir: BaseDirectory.AppLocalData });
          console.info("[MyPod CSV] staging copy succeeded", { build: CSV_PREVIEW_BUILD });
        } catch (error) {
          console.error("[MyPod CSV] staging copy failed", { build: CSV_PREVIEW_BUILD, error: String(error) });
          throw new Error(`CSV file access failed: ${error instanceof Error ? error.message : String(error)}`);
        }
        const preview = await dependencies.invoke<SpotifyCsvPreview>("preview_spotify_csv", { importId });
        console.info("[MyPod CSV] native preview returned", { build: CSV_PREVIEW_BUILD, sourceRecords: preview.summary.source_records, complete: preview.complete, fileErrors: preview.file_errors.length });
        if (preview.summary.source_records > MAX_SPOTIFY_CSV_RECORDS) {
          throw new Error("The native preview returned an invalid record count.");
        }
        return { status: "previewed", preview, importId };
      } finally {
        try {
          await dependencies.remove(stagedPath, { baseDir: BaseDirectory.AppLocalData });
        } catch {
          // Native command cleanup is authoritative; this is a best-effort fallback.
        }
      }
    },
    listAudioMatchReview: (catalogId) => dependencies.invoke("list_audio_match_review", { catalogId }),
    setAudioMatch: (catalogId, trackId) => dependencies.invoke("set_audio_match", { catalogId, trackId }),
    confirmSpotifyImport: (importId) => dependencies.invoke("confirm_spotify_import", { importId }),
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
