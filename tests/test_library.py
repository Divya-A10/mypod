import asyncio

import httpx

from backend.app.api.library import get_settings
from backend.app.config import Settings
from backend.app.database import get_track, list_tracks
from backend.app.library import scanner
from backend.app.library import metadata
from backend.app.library.scanner import ScanPathError, scan_directory
from backend.app.main import app


def settings_for(tmp_path, roots=None, managed=None):
    return Settings(
        database_path=tmp_path / "db" / "library.sqlite3",
        managed_music_dir=managed or tmp_path / "managed",
        permitted_scan_roots=tuple(roots if roots is not None else [tmp_path / "music"]),
    )


def fake_metadata(path):
    return {"title": path.stem.title(), "artist": "Test Artist", "album": "Test Album", "duration": 123.5}


def test_scan_is_idempotent_and_updates_changed_files(tmp_path, monkeypatch):
    root = tmp_path / "music"
    root.mkdir()
    track_path = root / "track.mp3"
    track_path.write_bytes(b"first")
    monkeypatch.setattr(scanner, "read_metadata", fake_metadata)
    settings = settings_for(tmp_path)

    assert scan_directory(root, settings) == {"imported": 1, "updated": 0, "skipped": 0, "failed": 0, "missing": 0}
    assert scan_directory(root, settings) == {"imported": 0, "updated": 0, "skipped": 1, "failed": 0, "missing": 0}

    track_path.write_bytes(b"changed size")
    assert scan_directory(root, settings)["updated"] == 1
    tracks, total = list_tracks(settings.database_path, limit=10, offset=0)
    assert total == 1
    assert tracks[0]["storage_type"] == "external"
    assert tracks[0]["title"] == "Track"
    assert get_track(settings.database_path, tracks[0]["id"])["file_size"] == len(b"changed size")


def test_scan_skips_symlinks_and_reports_missing_without_deleting(tmp_path, monkeypatch):
    root = tmp_path / "music"
    root.mkdir()
    audio = root / "song.flac"
    audio.write_bytes(b"audio")
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "linked.ogg").write_bytes(b"audio")
    (root / "linked-dir").symlink_to(outside, target_is_directory=True)
    monkeypatch.setattr(scanner, "read_metadata", fake_metadata)
    settings = settings_for(tmp_path)

    assert scan_directory(root, settings)["imported"] == 1
    audio.unlink()
    result = scan_directory(root, settings)
    assert result["missing"] == 1
    tracks, total = list_tracks(settings.database_path, limit=10, offset=0)
    assert total == 1
    assert tracks[0]["is_missing"] == 1


def test_scan_requires_resolved_path_within_permitted_roots(tmp_path):
    allowed = tmp_path / "allowed"
    allowed.mkdir()
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    settings = settings_for(tmp_path, roots=[allowed])

    try:
        scan_directory(elsewhere, settings)
    except ScanPathError as exc:
        assert "permitted scan root" in str(exc)
    else:
        raise AssertionError("outside scan root should be rejected")


def test_malformed_metadata_falls_back_to_filename(tmp_path, monkeypatch):
    path = tmp_path / "unknown-tags.mp3"
    monkeypatch.setattr(metadata, "MutagenFile", lambda *args, **kwargs: None)

    assert metadata.read_metadata(path) == {
        "title": "unknown-tags", "artist": None, "album": None, "duration": None,
    }


def test_scan_rejects_symlinked_scan_directory(tmp_path):
    allowed = tmp_path / "allowed"
    allowed.mkdir()
    alias = tmp_path / "alias"
    alias.symlink_to(allowed, target_is_directory=True)

    try:
        scan_directory(alias, settings_for(tmp_path, roots=[allowed]))
    except ScanPathError as exc:
        assert "symbolic link" in str(exc)
    else:
        raise AssertionError("symlinked scan root should be rejected")


def test_api_manual_scan_and_track_listing(tmp_path, monkeypatch):
    root = tmp_path / "music"
    root.mkdir()
    (root / "api.wav").write_bytes(b"audio")
    settings = settings_for(tmp_path)
    monkeypatch.setattr(scanner, "read_metadata", fake_metadata)
    monkeypatch.setitem(app.dependency_overrides, get_settings, lambda: settings)

    async def exercise_api():
        transport = httpx.ASGITransport(app=app)
        async with httpx.AsyncClient(transport=transport, base_url="http://test") as client:
            rejected = await client.post("/scan", json={"directory": str(tmp_path)})
            scanned = await client.post("/scan", json={"directory": str(root)})
            listing = await client.get("/tracks?limit=5&offset=0")
            detail = await client.get("/tracks/1")
            health = await client.get("/health")
            return rejected, scanned, listing, detail, health

    rejected, scanned, listing, detail, health = asyncio.run(exercise_api())
    assert rejected.status_code == 400
    assert scanned.json()["imported"] == 1
    assert listing.json()["total"] == 1
    assert detail.json()["title"] == "Api"
    assert health.json() == {"status": "ok"}
