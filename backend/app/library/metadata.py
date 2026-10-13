"""Read common audio tags without loading audio data into memory."""

from pathlib import Path

from mutagen import File as MutagenFile, MutagenError


SUPPORTED_FORMATS = {".mp3", ".flac", ".m4a", ".ogg", ".wav"}


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
