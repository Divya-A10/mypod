import asyncio
import os
from pathlib import Path
import wave

import httpx
import pytest

from backend.app.api.library import get_settings
from backend.app.config import Settings
from backend.app.database import connect
from backend.app.library import audio
from backend.app.main import app


def _write_synthetic_wav(path: Path) -> bytes:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(8000)
        output.writeframes(b"\x00\x00" * 32)
    return path.read_bytes()


def _register_track(tmp_path: Path, track_path: Path, *, is_missing: int = 0) -> Settings:
    database_path = tmp_path / "test-library.sqlite3"
    with connect(database_path) as connection:
        connection.execute(
            "INSERT INTO tracks (path, storage_type, title, format, file_size, modified_at, is_missing) "
            "VALUES (?, 'external', 'Synthetic fixture', 'wav', ?, 0, ?)",
            (str(track_path.resolve()), track_path.stat().st_size if track_path.exists() else 0, is_missing),
        )
        connection.commit()
    return Settings(
        database_path=database_path,
        managed_music_dir=tmp_path / "managed",
        permitted_scan_roots=(tmp_path / "music",),
    )


def _request(settings: Settings, url: str = "/tracks/1/audio", headers: dict[str, str] | None = None):
    monkeypatch = pytest.MonkeyPatch()
    monkeypatch.setitem(app.dependency_overrides, get_settings, lambda: settings)

    async def send():
        transport = httpx.ASGITransport(app=app)
        async with httpx.AsyncClient(transport=transport, base_url="http://test") as client:
            return await client.get(url, headers=headers)

    try:
        return asyncio.run(send())
    finally:
        monkeypatch.undo()


def test_audio_endpoint_returns_full_file_and_stream_headers(tmp_path):
    music = tmp_path / "music"
    track = music / "fixture.wav"
    payload = _write_synthetic_wav(track)
    settings = _register_track(tmp_path, track)

    response = _request(settings)

    assert response.status_code == 200
    assert response.content == payload
    assert response.headers["accept-ranges"] == "bytes"
    assert response.headers["content-length"] == str(len(payload))
    assert response.headers["content-type"] == "audio/wav"
    assert response.headers["cache-control"] == "no-store"
    assert response.headers["x-content-type-options"] == "nosniff"


@pytest.mark.parametrize(
    ("range_value", "expected_start", "expected_end"),
    [("bytes=0-9", 0, 9), ("bytes=10-", 10, None), ("bytes=-8", None, None)],
)
def test_audio_endpoint_serves_valid_byte_ranges(tmp_path, range_value, expected_start, expected_end):
    track = tmp_path / "music" / "fixture.wav"
    payload = _write_synthetic_wav(track)
    settings = _register_track(tmp_path, track)

    response = _request(settings, headers={"Range": range_value})

    if range_value == "bytes=-8":
        start, end = len(payload) - 8, len(payload) - 1
    else:
        start = expected_start
        end = expected_end if expected_end is not None else len(payload) - 1
    assert response.status_code == 206
    assert response.content == payload[start : end + 1]
    assert response.headers["content-range"] == f"bytes {start}-{end}/{len(payload)}"
    assert response.headers["content-length"] == str(end - start + 1)
    assert response.headers["accept-ranges"] == "bytes"


@pytest.mark.parametrize("range_value", ["bytes=abc-def", "items=0-5", "bytes=0-3,8-10", "bytes=-"])
def test_malformed_or_multiple_ranges_are_ignored(tmp_path, range_value):
    track = tmp_path / "music" / "fixture.wav"
    payload = _write_synthetic_wav(track)
    settings = _register_track(tmp_path, track)

    response = _request(settings, headers={"Range": range_value})

    assert response.status_code == 200
    assert response.content == payload
    assert "content-range" not in response.headers


@pytest.mark.parametrize("range_value", ["bytes=999999-", "bytes=9-4", "bytes=-0"])
def test_unsatisfiable_ranges_return_416(tmp_path, range_value):
    track = tmp_path / "music" / "fixture.wav"
    payload = _write_synthetic_wav(track)
    settings = _register_track(tmp_path, track)

    response = _request(settings, headers={"Range": range_value})

    assert response.status_code == 416
    assert response.content == b""
    assert response.headers["content-range"] == f"bytes */{len(payload)}"
    assert response.headers["content-length"] == "0"


def test_audio_endpoint_rejects_missing_and_unknown_tracks(tmp_path):
    music = tmp_path / "music"
    missing_file = music / "missing.wav"
    music.mkdir()
    settings = _register_track(tmp_path, missing_file, is_missing=1)

    missing = _request(settings)
    unknown = _request(settings, "/tracks/99/audio")
    invalid_id = _request(settings, "/tracks/0/audio")

    assert missing.status_code == 404
    assert unknown.status_code == 404
    assert invalid_id.status_code == 404


def test_audio_endpoint_rejects_file_removed_after_scanning(tmp_path):
    track = tmp_path / "music" / "fixture.wav"
    _write_synthetic_wav(track)
    settings = _register_track(tmp_path, track)
    track.unlink()

    response = _request(settings)

    assert response.status_code == 404


def test_audio_endpoint_rejects_non_regular_library_entry(tmp_path):
    track = tmp_path / "music" / "directory.wav"
    track.mkdir(parents=True)
    settings = _register_track(tmp_path, track)

    response = _request(settings)

    assert response.status_code == 404


@pytest.mark.parametrize("path_kind", ["outside", "file-symlink", "directory-symlink"])
def test_audio_endpoint_rejects_paths_outside_permitted_root(tmp_path, path_kind):
    music = tmp_path / "music"
    music.mkdir()
    outside = tmp_path / "outside"
    outside.mkdir()
    outside_file = outside / "fixture.wav"
    _write_synthetic_wav(outside_file)
    if path_kind == "outside":
        stored_path = outside_file
    elif path_kind == "file-symlink":
        stored_path = music / "linked.wav"
        stored_path.symlink_to(outside_file)
    else:
        linked_directory = music / "linked-directory"
        linked_directory.symlink_to(outside, target_is_directory=True)
        stored_path = linked_directory / "fixture.wav"
    settings = _register_track(tmp_path, stored_path)

    response = _request(settings)

    assert response.status_code == 404


def test_descriptor_relative_open_rejects_directory_symlink_replacement(tmp_path, monkeypatch):
    music = tmp_path / "music"
    nested = music / "nested"
    track = nested / "fixture.wav"
    _write_synthetic_wav(track)
    outside = tmp_path / "outside"
    outside.mkdir()
    _write_synthetic_wav(outside / "fixture.wav")
    settings = _register_track(tmp_path, track)
    original_open = os.open
    swapped = False

    def replace_before_component_open(path, flags, mode=0o777, *, dir_fd=None):
        nonlocal swapped
        if path == "nested" and dir_fd is not None and not swapped:
            nested.rename(music / "nested-original")
            nested.symlink_to(outside, target_is_directory=True)
            swapped = True
        return original_open(path, flags, mode, dir_fd=dir_fd)

    monkeypatch.setattr(audio.os, "open", replace_before_component_open)
    with pytest.raises(audio.AudioFileAccessError):
        audio.open_library_audio(str(track.resolve()), settings)
    assert swapped
