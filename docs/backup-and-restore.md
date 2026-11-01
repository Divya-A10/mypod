# MyPod iOS Backup and Restore

MyPod's iOS catalog is stored under the app's private data directory at `MyPod/library.sqlite3`. Imported audio is stored in `MyPod/Media`; in-progress audio copies are in `MyPod/Importing`. The catalog contains imported-track metadata, Spotify metadata-only catalog rows, import provenance, playlists, ordered playlist entries, and audio-match relationships. The existing app does not explicitly select a SQLite journal mode; SQLite's default is used, and the active mode is recorded in each backup manifest.

## Backup types

- **Database-only** contains a consistent SQLite snapshot. It does not contain audio and can be restored only when every referenced media file already exists with the expected content hash.
- **Complete** contains the SQLite snapshot plus regular files from the app-managed `Media` and `Importing` directories. This keeps managed audio and recoverable staged audio together with the catalog. Ephemeral Spotify CSV preview copies are excluded by filename without being opened or read.

Both use the versioned `MYPODBK1` single-file `.mypodbackup` format. The JSON manifest records format and schema versions, source journal mode, scope, file count, paths, lengths, and SHA-256 checksums. Payloads are streamed in 128 KiB chunks. SQLite's `VACUUM INTO` produces the snapshot and includes committed WAL content. A backup is written to app-private temporary files and published only after it is flushed; interruption may leave `.partial` and `.snapshot.sqlite3` artifacts in `MyPod/Backups`. They are retained and are not automatically deleted. Storage is preflighted, and export failures leave the completed internal bundle available for retry during the current app session.

The bundle is not encrypted. Store it in a location protected like the source device, and do not share it if its catalog metadata is private. MyPod does not upload backups to a server. The iOS Files picker can expose cloud-backed providers; choose an on-device Files location for local-only storage because a provider may synchronize files according to its own settings.

## Restore behavior

Selecting a bundle validates format/version, allowed relative paths, lengths, checksums, the restored database's SQLite integrity and foreign keys, schema compatibility, and media references before presenting the restore action. The user must then press the explicit replacement button. A complete restore merges verified media without deleting or overwriting existing files; identical same-path files are reused and different-content collisions are rejected. A database-only restore requires matching media already in `Media`.

The replacement database is copied and validated before an atomic same-volume rename. When the current database uses WAL, SQLite is asked to checkpoint and switch to DELETE journal mode before replacement; if sidecars remain, restore stops before switching. If interrupted before the database switch, the old catalog stays live. Newly merged files and private restore workspaces are retained rather than automatically cleaned. If interruption occurs after the validated atomic database rename, the restored database is live. These guarantees assume the app has exclusive access to its private catalog through its shared catalog lock; the app should be closed before manually copying its container.

Restoring replaces catalog, playlist, and metadata rows with the snapshot. Existing app-managed audio is retained, so files that were not part of the snapshot may remain as unreferenced files. Restore is not a merge of playlist/catalog history. Always keep an independent backup before restoration.

## Simulator schema-v5 migration verification

This procedure is for a human operator and has **not** been run as part of the backup implementation. It never requires editing the live database directly. Replace `<DEVICE-UUID>` with the simulator UUID and `<BACKUP-DIR>` with a new, access-controlled host directory. Keep MyPod closed while copying the container. Bundle identifier currently used by the app: `app.mypod.local`.

1. Confirm the intended simulator and app container:

   ```sh
   xcrun simctl list devices available
   xcrun simctl terminate <DEVICE-UUID> app.mypod.local 2>/dev/null || true
   xcrun simctl get_app_container <DEVICE-UUID> app.mypod.local data
   ```

2. Make an independent, read-only pre-migration copy. Set `CONTAINER` to the exact path printed above; do not point commands at any other app container.

   ```sh
   mkdir -p <BACKUP-DIR>/pre-v5
   ditto "$CONTAINER" <BACKUP-DIR>/pre-v5/container-copy
   chmod -R a-w <BACKUP-DIR>/pre-v5/container-copy
   ```

3. Locate the copied database and inspect only that copy:

   ```sh
   find <BACKUP-DIR>/pre-v5/container-copy -type f -name library.sqlite3 -print
   DB_COPY="<paste-the-library.sqlite3-path-from-the-copy>"
   sqlite3 "$DB_COPY" <<'SQL'
   PRAGMA integrity_check;
   PRAGMA foreign_key_check;
   PRAGMA user_version;
   SELECT 'imported_tracks', count(*) FROM imported_tracks;
   SELECT 'catalog_tracks', count(*) FROM catalog_tracks;
   SELECT 'spotify_imports', count(*) FROM spotify_imports;
   SELECT 'playlists', count(*) FROM playlists;
   SELECT 'playlist_entries', count(*) FROM playlist_entries;
   SQL
   find <BACKUP-DIR>/pre-v5/container-copy -type f -path '*/MyPod/Media/*' -exec shasum -a 256 {} + | sort
   ```

   Save the counts and media hash inventory locally. Confirm `integrity_check` returns `ok`, `foreign_key_check` returns no rows, and the copy is independent from the simulator container. Do not print track names or personal metadata into logs.

4. Confirm recoverability before updating: use a disposable simulator/app container or the synthetic restore suite to restore a copy of the backup into a separate temporary catalog root. Run `PRAGMA integrity_check`, `PRAGMA foreign_key_check`, compare the recorded table counts, and compare media hashes. Never test restore against the live container.

5. Install/launch the updated schema-v5 app on the same simulator without uninstalling it or clearing app data. Allow the normal additive migration to run, then close MyPod cleanly.

6. Copy the post-migration app data container to a second new host directory. Find its `library.sqlite3` and rerun the same `sqlite3` commands against this **copy**, not the live database. Confirm `user_version` is 5, integrity is `ok`, foreign-key check returns no rows, and the five table counts match the pre-migration copy unless the migration explicitly documents a metadata-only transformation. Run the same `find ... -exec shasum` command on the post-migration copy; compare inventories and verify that every prior media file still exists with the same hash.

7. Keep both independent copies until the real-device backup and restore procedure has also been demonstrated. If counts, integrity, or media hashes differ unexpectedly, stop and preserve both copies; do not retry migration or restore over the simulator.

Avoid opening the live SQLite database while the app is running. SQLite tools may create or update journal sidecars even for operations that appear read-only.
