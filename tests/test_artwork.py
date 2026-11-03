from types import SimpleNamespace

import pytest
from mutagen.id3 import APIC, ID3
from mutagen.mp4 import MP4Cover

from backend.app.api.library import track_artwork
from backend.app.config import Settings
from backend.app.database import connect
from backend.app.library import metadata


PNG = b"\x89PNG\r\n\x1a\nsynthetic-png-cover"
JPEG = b"\xff\xd8\xffsynthetic-jpeg-cover"


class FakeTags(dict):
    def __init__(self, values=None, frames=()):
        super().__init__(values or {})
        self._frames = frames

    def getall(self, key):
        return [frame for frame in self._frames if frame.FrameID == key]

    def __bool__(self):
        return bool(len(self) or self._frames)


@pytest.mark.parametrize(
    ("tags", "expected"),
    [
        (FakeTags(frames=[SimpleNamespace(FrameID="APIC", data=PNG)]), (PNG, "image/png")),
        (FakeTags({"covr": [JPEG]}), (JPEG, "image/jpeg")),
        (FakeTags(), None),
        (FakeTags({"covr": [b"not an image"]}), None),
        (FakeTags({"covr": [PNG + b"x" * (metadata.MAX_EMBEDDED_ARTWORK_BYTES + 1)]}), None),
    ],
)
def test_embedded_artwork_handles_id3_mp4_missing_invalid_and_oversized(tags, expected, monkeypatch, tmp_path):
    source_path = tmp_path / "synthetic.mp3"
    source_path.write_bytes(b"synthetic audio bytes")
    monkeypatch.setattr(metadata, "MutagenFile", lambda fileobj, easy: SimpleNamespace(tags=tags))
    with source_path.open("rb") as source:
        assert metadata.read_embedded_artwork(source) == expected


def test_real_mutagen_id3_apic_and_mp4_covr_objects_are_supported(monkeypatch, tmp_path):
    source_path = tmp_path / "synthetic.audio"
    source_path.write_bytes(b"synthetic bytes")
    id3 = ID3()
    id3.add(APIC(encoding=3, mime="image/png", type=3, desc="cover", data=PNG))
    monkeypatch.setattr(metadata, "MutagenFile", lambda fileobj, easy: SimpleNamespace(tags=id3))
    with source_path.open("rb") as source:
        assert metadata.read_embedded_artwork(source) == (PNG, "image/png")

    covr = MP4Cover(JPEG, imageformat=MP4Cover.FORMAT_JPEG)
    monkeypatch.setattr(metadata, "MutagenFile", lambda fileobj, easy: SimpleNamespace(tags={"covr": [covr]}))
    with source_path.open("rb") as source:
        assert metadata.read_embedded_artwork(source) == (JPEG, "image/jpeg")


def test_synthetic_mp3_with_and_without_apic_art(tmp_path):
    tagged = tmp_path / "tagged.mp3"
    untagged = tmp_path / "untagged.mp3"
    # Four synthetic MPEG-1 Layer III frames are enough for Mutagen to parse
    # the ID3 tag; the fixture contains no copyrighted media.
    frames = (b"\xff\xfb\x90\x64" + bytes(413)) * 4
    tagged.write_bytes(frames)
    untagged.write_bytes(frames)
    id3 = ID3()
    id3.add(APIC(encoding=3, mime="image/png", type=3, desc="cover", data=PNG))
    id3.save(tagged)
    with tagged.open("rb") as source:
        assert metadata.read_embedded_artwork(source) == (PNG, "image/png")
    with untagged.open("rb") as source:
        assert metadata.read_embedded_artwork(source) is None


def test_track_artwork_uses_only_exact_album_artist_fallback(tmp_path, monkeypatch):
    music = tmp_path / "music"
    music.mkdir()
    files = {
        "current.mp3": b"track-without-art",
        "same-album.mp3": b"same-album-cover",
        "different-artist.mp3": b"different-artist-cover",
    }
    for name, contents in files.items():
        (music / name).write_bytes(contents)
    settings = Settings(
        database_path=tmp_path / "db" / "library.sqlite3",
        managed_music_dir=tmp_path / "managed",
        permitted_scan_roots=(music,),
    )
    with connect(settings.database_path) as connection:
        for name, artist in (
            ("current.mp3", "Artist"),
            ("same-album.mp3", "Artist"),
            ("different-artist.mp3", "Other Artist"),
        ):
            connection.execute(
                "INSERT INTO tracks(path,storage_type,title,artist,album,format,file_size,modified_at) "
                "VALUES(?, 'external', ?, ?, 'Exact Album', 'mp3', 20, 1)",
                (str(music / name), name, artist),
            )
        connection.commit()

    def extract(source):
        contents = source.read()
        if contents == b"same-album-cover":
            return PNG, "image/png"
        if contents == b"different-artist-cover":
            return JPEG, "image/jpeg"
        return None

    monkeypatch.setattr("backend.app.api.library.read_embedded_artwork", extract)
    response = track_artwork(1, settings)
    assert response.status_code == 200
    assert response.media_type == "image/png"
    assert response.body == PNG
    assert response.headers["x-content-type-options"] == "nosniff"


def test_artwork_lookup_never_accepts_a_filesystem_path(tmp_path):
    from fastapi import HTTPException

    with pytest.raises(HTTPException):
        track_artwork(0, Settings(tmp_path / "db.sqlite", tmp_path / "media", ()))
