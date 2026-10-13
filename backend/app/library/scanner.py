"""Manual, bounded directory scanner for local audio files."""

from datetime import datetime, timezone
import os
from pathlib import Path
import sqlite3
from typing import Iterator

from ..config import Settings
from ..database import connect
from .metadata import SUPPORTED_FORMATS, read_metadata


class ScanPathError(ValueError):
    pass


def _validate_scan_path(directory: str | Path, roots: tuple[Path, ...]) -> Path:
    if not str(directory).strip():
        raise ScanPathError("Scan directory is required")
    supplied = Path(directory).expanduser()
    if supplied.is_symlink():
        raise ScanPathError("Scan directory cannot be a symbolic link")
    try:
        requested = supplied.resolve(strict=True)
    except (OSError, RuntimeError) as exc:
        raise ScanPathError("Scan directory does not exist or cannot be resolved") from exc
    if not requested.is_dir():
        raise ScanPathError("Scan path must be a directory")
    if not roots:
        raise ScanPathError("No permitted scan roots are configured")
    permitted = False
    for configured_root in roots:
        try:
            root = configured_root.expanduser().resolve(strict=True)
        except (OSError, RuntimeError):
            continue
        if not root.is_dir():
            continue
        try:
            requested.relative_to(root)
            permitted = True
            break
        except ValueError:
            continue
    if not permitted:
        raise ScanPathError("Scan directory must resolve within a configured permitted scan root")
    return requested


def _audio_paths(root: Path) -> Iterator[Path | None]:
    """Yield supported regular files without traversing symlinks."""
    pending = [root]
    while pending:
        current = pending.pop()
        try:
            with os.scandir(current) as entries:
                for entry in entries:
                    try:
                        if entry.is_symlink():
                            continue
                        if entry.is_dir(follow_symlinks=False):
                            pending.append(Path(entry.path))
                        elif entry.is_file(follow_symlinks=False) and Path(entry.name).suffix.lower() in SUPPORTED_FORMATS:
                            yield Path(entry.path)
                    except OSError:
                        yield None
                        continue
        except OSError:
            yield None
            continue


def scan_directory(directory: str | Path, settings: Settings) -> dict[str, int]:
    root = _validate_scan_path(directory, settings.permitted_scan_roots)
    counts = {"imported": 0, "updated": 0, "skipped": 0, "failed": 0, "missing": 0}
    now = datetime.now(timezone.utc).isoformat()
    seen: set[str] = set()
    try:
        managed_root = settings.managed_music_dir.expanduser().resolve()
    except (OSError, RuntimeError):
        managed_root = settings.managed_music_dir.expanduser().absolute()
    with connect(settings.database_path) as connection:
        for path in _audio_paths(root):
            if path is None:
                counts["failed"] += 1
                continue
            try:
                canonical_path = path.resolve(strict=True)
                canonical_path.relative_to(root)
                canonical = str(canonical_path)
                seen.add(canonical)
                stat = path.stat()
                metadata = read_metadata(path)
                previous = connection.execute(
                    "SELECT id, file_size, modified_at FROM tracks WHERE path = ?", (canonical,)
                ).fetchone()
                values = (
                    metadata["title"], metadata["artist"], metadata["album"], metadata["duration"],
                    path.suffix.lower().lstrip("."), stat.st_size, stat.st_mtime,
                )
                try:
                    path.relative_to(managed_root)
                    storage_type = "managed"
                except ValueError:
                    storage_type = "external"
                if previous is None:
                    connection.execute(
                        "INSERT INTO tracks (path, storage_type, title, artist, album, duration, format, file_size, modified_at, updated_at, is_missing) "
                        "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)",
                        (canonical, storage_type, *values, now),
                    )
                    counts["imported"] += 1
                elif previous["file_size"] == stat.st_size and previous["modified_at"] == stat.st_mtime:
                    connection.execute("UPDATE tracks SET is_missing = 0 WHERE path = ?", (canonical,))
                    counts["skipped"] += 1
                else:
                    connection.execute(
                        "UPDATE tracks SET title=?, artist=?, album=?, duration=?, format=?, file_size=?, modified_at=?, updated_at=?, is_missing=0 WHERE path=?",
                        (*values, now, canonical),
                    )
                    counts["updated"] += 1
            except (OSError, ValueError, sqlite3.Error):
                counts["failed"] += 1

        if counts["failed"] == 0:
            prefix = str(root).rstrip(os.sep) + os.sep
            rows = connection.execute(
                "SELECT path FROM tracks WHERE substr(path, 1, ?) = ?", (len(prefix), prefix)
            ).fetchall()
            for row in rows:
                if row["path"] not in seen:
                    connection.execute("UPDATE tracks SET is_missing = 1 WHERE path = ?", (row["path"],))
                    counts["missing"] += 1
        connection.commit()
    return counts
