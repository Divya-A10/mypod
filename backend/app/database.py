"""SQLite persistence for indexed audio tracks."""

from contextlib import contextmanager
import sqlite3
from pathlib import Path
from typing import Iterator


SCHEMA = """
CREATE TABLE IF NOT EXISTS tracks (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    storage_type TEXT NOT NULL CHECK (storage_type IN ('external', 'managed')),
    title TEXT NOT NULL,
    artist TEXT,
    album TEXT,
    duration REAL,
    format TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    modified_at REAL NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    is_missing INTEGER NOT NULL DEFAULT 0 CHECK (is_missing IN (0, 1))
);
CREATE INDEX IF NOT EXISTS idx_tracks_artist_album ON tracks (artist, album);
CREATE INDEX IF NOT EXISTS idx_tracks_path ON tracks (path);
"""


@contextmanager
def connect(database_path: Path) -> Iterator[sqlite3.Connection]:
    database_path.parent.mkdir(parents=True, exist_ok=True)
    connection = sqlite3.connect(database_path)
    connection.row_factory = sqlite3.Row
    try:
        connection.executescript(SCHEMA)
        yield connection
    finally:
        connection.close()


def list_tracks(database_path: Path, limit: int, offset: int) -> tuple[list[dict], int]:
    with connect(database_path) as connection:
        total = connection.execute("SELECT COUNT(*) FROM tracks").fetchone()[0]
        rows = connection.execute(
            "SELECT * FROM tracks ORDER BY artist COLLATE NOCASE, album COLLATE NOCASE, title COLLATE NOCASE LIMIT ? OFFSET ?",
            (limit, offset),
        ).fetchall()
        return [dict(row) for row in rows], total


def get_track(database_path: Path, track_id: int) -> dict | None:
    with connect(database_path) as connection:
        row = connection.execute("SELECT * FROM tracks WHERE id = ?", (track_id,)).fetchone()
        return dict(row) if row else None
