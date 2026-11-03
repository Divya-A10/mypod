"""Read common audio tags without loading audio data into memory."""

from pathlib import Path
from typing import BinaryIO

from mutagen import File as MutagenFile, MutagenError


SUPPORTED_FORMATS = {".mp3", ".flac", ".m4a", ".ogg", ".wav"}
MAX_EMBEDDED_ARTWORK_BYTES = 8 * 1024 * 1024

_ARTWORK_SIGNATURES = (
    (b"\xff\xd8\xff", "image/jpeg"),
    (b"\x89PNG\r\n\x1a\n", "image/png"),
    (b"GIF87a", "image/gif"),
    (b"GIF89a", "image/gif"),
)


def _image_type(data: bytes) -> str | None:
    for signature, mime_type in _ARTWORK_SIGNATURES:
        if data.startswith(signature):
            return mime_type
    if len(data) >= 12 and data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        return "image/webp"
    return None


def read_embedded_artwork(fileobj: BinaryIO) -> tuple[bytes, str] | None:
    """Read one bounded embedded cover without exposing or changing its source path.

    Mutagen handles ID3 APIC and MP4/M4A covr frames. The returned bytes are
    validated by signature, and oversized or unsupported images are ignored.
    """
    try:
        fileobj.seek(0)
        audio = MutagenFile(fileobj, easy=False)
        tags = getattr(audio, "tags", None) if audio is not None else None
        if not tags:
            return None

        candidates = []
        getall = getattr(tags, "getall", None)
        if callable(getall):
            candidates.extend(frame.data for frame in getall("APIC") if getattr(frame, "data", None))
        covers = tags.get("covr") if hasattr(tags, "get") else None
        if covers:
            candidates.extend(bytes(cover) for cover in covers)
        for data in candidates:
            if not data or len(data) > MAX_EMBEDDED_ARTWORK_BYTES:
                continue
            mime_type = _image_type(data)
            if mime_type:
                return data, mime_type
        return None
    except (MutagenError, OSError, ValueError, EOFError, TypeError):
        return None


def _first_tag(tags, key: str) -> str | None:
    if not tags:
        return None
    value = tags.get(key)
    if value is None:
        return None
    if isinstance(value, (list, tuple)):
        value = value[0] if value else None
    if value is None:
        return None
    return str(value).strip() or None


def read_metadata(path: Path) -> dict:
    try:
        audio = MutagenFile(path, easy=True)
    except (MutagenError, OSError, ValueError, EOFError):
        audio = None
    if audio is None:
        return {"title": path.stem, "artist": None, "album": None, "duration": None}
    info = getattr(audio, "info", None)
    length = getattr(info, "length", None)
    return {
        "title": _first_tag(getattr(audio, "tags", None), "title") or path.stem,
        "artist": _first_tag(getattr(audio, "tags", None), "artist"),
        "album": _first_tag(getattr(audio, "tags", None), "album"),
        "duration": float(length) if length is not None else None,
    }
