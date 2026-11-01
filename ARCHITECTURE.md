# MyPod architecture

The desktop React/Tauri shell uses a localhost FastAPI/SQLite library and HTML media playback. The iOS adapter invokes Rust commands against an app-private SQLite catalog; it does not call the Mac backend. Native AVPlayer resolves audio by local track ID through the existing validated managed-media resolver.

The iOS schema migrates transactionally through `PRAGMA user_version`. v1 contains managed audio; v2 adds playlists and snapshot entries; v3 adds metadata recordings, source imports and lossless provenance rows; v4 adds supplemental embedded tags, verified matches and manual unlink suppression; v5 adds a unique local identifier to catalog recordings. Metadata recordings extend the existing playlist-entry model through a nullable catalog reference, while playable IDs still reference imported audio. No fake media paths or competing playback implementation are introduced.

The CSV preview parser remains independent of database code. A single bounded preview is held in native memory, keyed by the picker token. Explicit confirmation imports that trusted preview in one transaction. The frontend cannot submit edited metadata as a confirmed preview. Closing preview or cancelling the picker writes no catalog rows. Staged CSV copies are deleted immediately after parsing. Metadata provenance persists locally only after confirmation.

See `docs/spotify-import-and-matching.md` for policies and validation.
