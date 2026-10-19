# myPod

myPod (LocalPod) is a local-first music player and personal recommendation system. The backend indexes local audio into SQLite. A responsive React interface and Tauri desktop shell are in place as an initial UI pass. Playback, playlist and favorites APIs, artwork delivery, managed-file importing, and recommendation features are not implemented yet.

## Backend setup

The backend uses FastAPI for HTTP endpoints, Uvicorn to serve the app, and Mutagen to read local audio metadata. SQLite is included with Python. A virtual environment (`.venv`) keeps this project's Python packages separate from your system Python and other projects. The development extras add pytest and HTTPX for isolated in-process API tests.

From the repository root, create and activate the virtual environment, then install the backend and its development dependencies:

```sh
python3 -m venv .venv
source .venv/bin/activate
python -m pip install -e 'backend[dev]'
```

Start the API:

```sh
uvicorn app.main:app --app-dir backend --reload
```

The API binds to localhost by default. The health check is available at <http://127.0.0.1:8000/health> and returns `{"status":"ok"}`.

## Desktop and responsive UI

For the complete startup instructions for the backend, browser frontend, macOS app, and iPhone Simulator, see [the development startup guide](docs/development-startup.md).

With the backend running, start the React preview from `apps/desktop`:

```sh
cd /Users/divyaavanigadda/Desktop/mypod/apps/desktop
npm run dev
```

The responsive UI is available at <http://127.0.0.1:1420> and uses the track-list API through Vite's local development proxy. Desktop audio playback uses the shared playback controller and HTML media element. The iOS Simulator app has an on-device import/catalog path, while native iOS audio playback is not implemented yet.

## Local library configuration

Set these environment variables before starting the API:

| Variable | Default | Purpose |
| --- | --- | --- |
| `MYPOD_DATA_DIR` | `~/.localpod` | Default location for LocalPod data |
| `MYPOD_DATABASE_PATH` | `$MYPOD_DATA_DIR/library.sqlite3` | SQLite database file |
| `MYPOD_MUSIC_DIR` | `$MYPOD_DATA_DIR/music` | Reserved LocalPod-managed music directory; this stage does not copy files into it |
| `MYPOD_SCAN_ROOTS` | unset (no scans permitted) | Permitted scan root directories, separated by the platform path separator |

For example, in a shell session, allow scans only inside a directory you choose:

```sh
export MYPOD_SCAN_ROOTS="$HOME/Music/Library"
```

This configures permission; it does not start a scan. Each scan must be explicitly requested. The requested directory must exist and resolve within one of the configured roots. The scanner does not follow symbolic links. It only reads supported MP3, FLAC, M4A, OGG, and WAV metadata. It does not copy, move, rename, modify, or delete audio files. External indexed files and files in the configured managed directory are labeled separately in the library; managed imports are not implemented.

## Library API

- `GET /tracks?limit=50&offset=0` returns `{ "items": [...], "total": 0, "limit": 50, "offset": 0 }`. `limit` is 1–100.
- `GET /tracks/{id}` returns one track or `404` if it does not exist.
- `POST /scan` explicitly scans a permitted directory. Example request: `{ "directory": "/chosen/music/folder" }`. The response contains `imported` (new database records), `updated`, `skipped`, `failed`, and `missing` counts. Unchanged records are counted as skipped. Missing files remain in SQLite and are marked missing; they are never deleted. If traversal encounters failures, missing-file marking is deferred to avoid treating inaccessible files as deleted.

Nonexistent, inaccessible, or unpermitted scan roots return `400`; file or nested-directory read errors are counted as failures in the scan result. With no `MYPOD_SCAN_ROOTS` configured, scans are rejected. Keep the server on localhost; do not bind it to a public interface without adding authentication and network protections.

## Spotify CSV preview

`POST /imports/spotify/preview` accepts the currently supported CSV shape as a `text/csv` request body. It accepts UTF-8 with or without a BOM and requires these exact headers, in order: `Track name`, `Artist name`, `Album`, `Playlist name`, `Type`, `ISRC`, and `Spotify - id`. This endpoint previews and validates rows only; it does not import them or interpret source types as LocalPod entities.

Preview has hard limits of 10 MiB per request and 20,000 non-empty data records, excluding the header. Inputs at either limit are accepted. Inputs one byte or one data record over are rejected as a whole; no partial preview is returned for a limit violation. Completely empty records are ignored and counted in `ignored_empty_records`.

The response contains `complete`, parsed `columns`, aggregate `summary`, `file_errors`, every identifiable non-empty record in `rows`, and `duplicate_reports`. Each row includes its 1-based `source_record` ordinal after the header (including ignored empty records), physical line start/end, original parsed `source_values`, candidate `mapped` values, and row-level `errors` and `warnings`. Duplicate reports distinguish exact duplicate rows, repeated IDs within a playlist, IDs shared across playlists, and conflicting metadata for a shared ID; occurrences are never deduplicated. If a fatal CSV syntax error prevents reliable recovery, the response is marked `complete: false`, includes the recoverable rows and a file-level parse error with its physical line span, and does not claim the remainder was previewed.

The preview parser is read-only by architecture: it operates on the request body in memory and does not import or call database connection, initialization, migration, or write code. It does not create a database file or write the CSV to disk. This behavior is covered by synthetic tests; personal exports are not test fixtures.

Run the tests from the repository root. They use temporary folders and temporary databases with mocked metadata, not a personal music library or Spotify export:

```sh
python -m pytest
```
