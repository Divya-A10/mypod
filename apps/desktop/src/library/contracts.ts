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

/** Platform-neutral library contract used by shared UI code. */
export interface LibraryAdapter {
  listTracks(offset?: number, signal?: AbortSignal): Promise<TrackResponse>;
  getTrack(trackId: number, signal?: AbortSignal): Promise<Track>;
  importAudio?(): Promise<AudioImportResult>;
  previewSpotifyCsv?(): Promise<SpotifyCsvPreviewResult>;
  playlists?: PlaylistLibraryAdapter;
}

export type AudioImportResult =
  | { status: "cancelled" }
  | { status: "imported" | "duplicate"; track: Track };

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
  | { status: "previewed"; preview: SpotifyCsvPreview };
