/** Metadata exposed by every platform's local library implementation. */
export type Track = {
  id: number;
  title: string;
  artist: string | null;
  album: string | null;
  duration: number | null;
  format: string;
  file_size: number;
  modified_at: number;
  created_at: string;
  updated_at: string;
  is_missing: number;
};

export type TrackResponse = {
  items: Track[];
  total: number;
  limit: number;
  offset: number;
};

export type FolderScanResult = {
  imported: number;
  updated: number;
  skipped: number;
  failed: number;
  missing: number;
};

/** Playlist identity is its local catalog ID, never its display name. */
export type Playlist = {
  id: number;
  name: string;
  track_count: number;
  created_at: number;
  updated_at: number;
  source_playlist_label: string | null;
  source_playlist_identifier: string | null;
};

/** Snapshots and opaque source fields keep unmatched imported rows reviewable. */
export type PlaylistEntry = {
  id: number;
  playlist_id: number;
  position: number;
  track_id: number | null;
  title: string;
  artist: string | null;
  album: string | null;
  source_type_label: string | null;
  source_identifier: string | null;
  catalog_id?: number | null;
};

export interface PlaylistLibraryAdapter {
  listPlaylists(): Promise<Playlist[]>;
  createPlaylist(name: string): Promise<Playlist>;
  renamePlaylist(playlistId: number, name: string): Promise<Playlist>;
  deletePlaylist(playlistId: number): Promise<void>;
  listEntries(playlistId: number): Promise<PlaylistEntry[]>;
  addTrack(playlistId: number, trackId: number): Promise<PlaylistEntry>;
  removeEntry(playlistId: number, entryId: number): Promise<void>;
  reorderEntries(playlistId: number, entryIds: number[]): Promise<PlaylistEntry[]>;
}

export type BackupScope = "database_only" | "complete";
export type BackupSummary = {
  format_version: number;
  schema_version: number;
  journal_mode: string;
  scope: BackupScope;
  file_count: number;
  total_bytes: number;
  sha256: string;
};
export interface BackupAdapter {
  createBackup(scope: BackupScope): Promise<{ status: "cancelled" } | { status: "created"; token: string; summary: BackupSummary; exportError?: string }>;
  exportBackup(token: string): Promise<boolean>;
  inspectBackup(): Promise<{ status: "cancelled" } | { status: "ready"; token: string; summary: BackupSummary }>;
  restoreBackup(token: string): Promise<BackupSummary>;
}

/** Platform-neutral library contract used by shared UI code. */
export interface LibraryAdapter {
  listTracks(offset?: number, signal?: AbortSignal): Promise<TrackResponse>;
  getTrack(trackId: number, signal?: AbortSignal): Promise<Track>;
  importAudio?(): Promise<AudioImportResult>;
  scanAudioFolder?(): Promise<{ status: "cancelled" } | { status: "scanned"; result: FolderScanResult }>;
  previewSpotifyCsv?(file?: File): Promise<SpotifyCsvPreviewResult>;
  listAudioMatchReview?(catalogId: number): Promise<AudioMatchReview[]>;
  setAudioMatch?(catalogId: number, trackId: number | null): Promise<void>;
  confirmSpotifyImport?(importId: string): Promise<{ already_imported: boolean; playlists: number; entries: number; preserved_other_rows: number }>;
  backups?: BackupAdapter;
  playlists?: PlaylistLibraryAdapter;
}

export type AudioImportResult =
  | { status: "cancelled" }
  | { status: "imported" | "duplicate"; track: Track; metadataWarning?: string | null };

export type SpotifyCsvIssue = {
  code: string;
  message?: string;
  columns?: string[];
  field?: string;
  value?: string;
  expected?: number;
  actual?: number;
  physical_line_start?: number;
  physical_line_end?: number;
  max_bytes?: number;
  actual_bytes?: number;
  max_records?: number;
  observed_records?: number;
};

export type SpotifyCsvRow = {
  source_record: number;
  physical_line_start: number;
  physical_line_end: number;
  source_values: string[];
  mapped: {
    track_name: string | null;
    artist_name: string | null;
    album: string | null;
    playlist_name: string | null;
    source_type: string | null;
    isrc: string | null;
    spotify_id: string | null;
  };
  errors: SpotifyCsvIssue[];
  warnings: SpotifyCsvIssue[];
  duplicate_reports: string[];
};

export type SpotifyCsvPreview = {
  complete: boolean;
  columns: string[];
  summary: {
    source_records: number;
    preview_rows: number;
    ignored_empty_records: number;
    ignored_empty_spans: Array<{
      source_record_start: number;
      source_record_end: number;
      physical_line_start: number;
      physical_line_end: number;
    }>;
    rows_with_errors: number;
    rows_with_warnings: number;
  };
  file_errors: SpotifyCsvIssue[];
  rows: SpotifyCsvRow[];
  duplicate_reports: Array<{
    report_id: string;
    category: string;
    source_records: number[];
    differing_fields?: string[];
  }>;
};

export type SpotifyCsvPreviewResult =
  | { status: "cancelled" }
  | { status: "previewed"; preview: SpotifyCsvPreview; importId?: string };

export type AudioMatchReview = {
  catalog_id: number; title: string; artist: string | null; album: string | null;
  track_id: number | null; method: string | null; available: boolean;
  candidates: Array<{ track_id: number; title: string; artist: string | null; album: string | null; reason: string; available: boolean }>;
};

export type IosRuntimeDiagnostics = { bridge_works: boolean; native_build_id: string; database_schema_version: number | null; migration_status: string; csv_selection_returned_path: boolean; selected_file_exists: boolean | null; staging_succeeded: boolean; preview_succeeded: boolean; failure_stage: string; sanitized_error: string | null; };
