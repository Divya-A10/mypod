# Spotify metadata import and local audio matching

## Architecture assessment

Stage 3A already provided a bounded Rust CSV preview, an iOS Files-picker adapter, a separate read-only Python preview, SQLite v2 playlists with nullable audio IDs and metadata snapshots, app-private managed audio, and native AVPlayer playback. The architecture guide was empty and the README/startup guide understated the implemented playback. The desktop and iOS catalogs have different persistence boundaries; the iPhone feature extends the existing iOS playlist schema rather than routing mobile data through localhost.

## Import on iPhone

1. Rebuild/install the updated iOS app using the development startup guide. A running older build will still only preview CSVs.
2. Make the CSV available in the iPhone Files app (for example, copy it into iCloud Drive or an On My iPhone location). MyPod cannot access a Mac Desktop path directly. For the simulator, use a location its Files picker can actually open; dragging a CSV onto Simulator does not guarantee it appears in Files.
3. In MyPod, open **Your Library → Preview Spotify CSV**, choose the CSV, and inspect counts, playlist groups, duplicate reports and validation warnings.
4. Tap **Confirm metadata and playlist import**. Closing the preview or cancelling selection does not import anything. Errors disable confirmation; native validation also rejects malformed/incomplete previews.
5. Open **Your Library → Playlists**. Imported cards are marked **Imported Spotify**, apart from cards marked **My playlist**. Entries initially show **Missing audio**.
6. Later, use **Import audio** to choose a supported local audio file. One managed copy serves every matching playlist entry. The importer reads embedded metadata from its staged copy without changing the original audio.
7. Open a playlist and tap **Review audio** on an imported recording. Inspect suggested candidates and their evidence, select the same recording/version, and tap **Confirm audio match**. Use **Load more** if the desired local track is not loaded in the selector. **Unlink audio** removes the association across all entries for that catalog recording and suppresses future automatic linking until you explicitly confirm another file. **Cancel** leaves the match unchanged.

## Schema and migration

Each migration runs in an immediate SQLite transaction and advances `PRAGMA user_version` only after success. Catalogs from newer app versions are rejected.

- v1/v2: existing `imported_tracks`, `playlists`, and `playlist_entries` remain intact.
- v3: adds `spotify_imports` (unique parsed-source SHA-256), `catalog_tracks` (internal integer identity and original Spotify/ISRC identifiers), and `spotify_rows` (batch, source ordinal, physical line spans, record type, original playlist label, catalog/entry references and losslessly length-framed original cells). Adds nullable `playlist_entries.catalog_id`. Playback `track_id` remains a foreign key to actual imported audio.
- v4: adds supplemental `audio_metadata`, `audio_matches` (one selected audio file per catalog recording, with automatic/manual provenance) and `match_suppressions`. Existing local track rows and metadata are not rewritten.
- v5: adds/backfills the unique `catalog_tracks.local_identifier` used by the on-device importer. This is additive and preserves all existing catalog and playlist rows.

Favorites retain their type and catalog relationship in `spotify_rows`; Album/Artist/unknown entities and unnamed playlist rows retain their original cells separately, without inventing playlist membership. There is currently no dedicated favorites/source-entity browser. They are not converted into ordinary playlists or discarded.

## Grouping, duplicates and order

Only exact `Type=Playlist` rows with a nonblank `Playlist name` create membership. The exact original label is the grouping key within a contiguous block of nonempty parsed records. A later block with the same label creates a separate internal playlist, including within the same import. This conservatively avoids joining separated same-named source playlists. Different exports/batches never append into existing user playlists.

No playlist IDs exist in the supported seven-column shape. Two same-named playlists that were flattened into one uninterrupted block cannot be distinguished. The preview explicitly warns about this ambiguity. The import does not claim to recover identities absent from the source. Long display labels are shortened to the existing 120-character display limit while full labels and independent internal identities remain preserved.

All occurrences are retained as distinct playlist entries, including exact duplicate rows. Catalog recordings are shared within an import only when the Spotify ID, title, artist, album and ISRC source fields all agree exactly. Conflicting versions remain separate, regardless of a shared source ID. Catalog IDs and playlist IDs are local integers, not source names or Spotify IDs.

Source ordinals and physical line spans persist in provenance. Initial playlist positions follow source row order, provisionally. There is no claim that CSV order is original Spotify order. User reorder operations change display positions without rewriting original source ordinals.

A length-framed SHA-256 fingerprint includes the ordered source columns and parsed cells. Reconfirming the same source is a no-op, including after playlist edits/deletion; it does not restore or overwrite those edits. Equivalent quoting/newline styles with the same parsed cells also count as the same source. Changed exports are independent new batches. This is intentionally conservative; it is not an incremental synchronization system.

## Matching policy

Lofty 0.22.4 reads supported embedded tags; cover art and audio properties are skipped, custom resolvers are disabled, and per-item allocation is capped at 2 MiB. Read failures preserve audio import and produce a review warning. Files without tags cannot qualify for automatic linking. Supplemental metadata enriches the local library presentation without rewriting legacy track metadata.

Automatic links require all of the following:

- A syntactically valid 12-character ISRC agrees after uppercasing and removing hyphens.
- Nonblank title, artist and album agree after case/whitespace normalization only. Version text and punctuation are retained, so live, remaster, acoustic and remix variants are not collapsed.
- Exactly one available local audio candidate and exactly one catalog identity satisfy the evidence.
- No manual match/unlink suppression exists for the catalog recording.

ISRC alone and filename guesses never qualify. Every audio/CSV import recomputes automatic associations; newly competing evidence removes automatic links and requires review. Manual links remain explicit user choices. Candidate review is computed from current title or ISRC evidence on demand, so missing/ambiguous catalog entries remain reviewable without a second queue database. The native commands validate selected IDs and file availability. Imported metadata alone never produces a playable audio ID.

Missing or removed files remain associated in match history but are displayed as missing, have no usable playlist play action, and are rejected by the existing native resolver. Re-selecting an identical previously imported file retains its local audio ID, repairs a missing managed copy, and supplements legacy tag metadata when absent. No playlist creates another audio copy.

## Validation

Run from repository root:

```sh
.venv/bin/python -m pytest -q
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib --offline
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --target aarch64-apple-ios-sim --lib --offline
```

Run from `apps/desktop`:

```sh
npm run build
node --test tests/*.test.mjs
```

Synthetic tests cover picker cancellation, explicit token-only confirmation, malformed previews, migration preserving legacy tracks, same-named blocks, repeated entries, non-playlist/unnamed rows, existing playlists, transactional rollback, reimport idempotency, metadata-only states, exact/ambiguous matching, version conflicts, manual confirmation/unlink, one audio file in multiple playlists and removed audio. The embedded-tag integration test constructs a synthetic WAV and writes tags only to that disposable fixture.

An ignored, opt-in test named `local_export_reconstruction` reads a CSV from `MYPOD_VALIDATION_CSV`, uses only a temporary catalog, checks source membership/order and reimport behavior, and verifies source bytes remain unchanged. It is tailored to the locally validated export's aggregate counts; ordinary automated tests require no personal dataset. No track/artist/playlist names are emitted. Personal CSVs and temporary catalogs are never copied into Git.

## Remaining manual checks and limits

The Rust iOS simulator target compile is a compile check, not an installed-app test. Simulator service access was unavailable in this execution environment. Manually verify Files-provider permissions, picker/preview cancellation, confirmation/result rendering at narrow iPhone widths, persistence after relaunch, match selection/unlink, and audible native playback with representative supported codecs. Metadata-tag parsing does not guarantee AVPlayer supports every codec/container accepted by the picker; existing native playback errors remain authoritative.

The new import and matching workflow is iPhone/on-device functionality. The desktop Python CSV endpoint remains preview-only, and desktop API/library behavior is preserved. No Spotify API, download service, telemetry or background folder scan was added. No commit or push is part of this work.
