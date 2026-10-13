"""Safe access to scanned audio files for local playback."""

from collections.abc import Iterator
import os
from pathlib import Path
import stat

from ..config import Settings
from .metadata import SUPPORTED_FORMATS


class AudioFileAccessError(ValueError):
    """Raised when a library audio file cannot be opened safely."""


CONTENT_TYPES = {
    ".flac": "audio/flac",
    ".m4a": "audio/mp4",
    ".mp3": "audio/mpeg",
    ".ogg": "audio/ogg",
    ".wav": "audio/wav",
}
_HAS_SECURE_DIR_FD = os.open in os.supports_dir_fd and hasattr(os, "O_NOFOLLOW") and hasattr(os, "O_DIRECTORY")


def _open_directory(path: Path) -> int:
    """Open a canonical directory one component at a time, refusing symlinks."""
    if not _HAS_SECURE_DIR_FD:
        raise AudioFileAccessError("Safe local audio access is unavailable on this platform")

    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | getattr(os, "O_CLOEXEC", 0)
    descriptor = os.open(path.anchor, flags)
    try:
        for component in path.parts[1:]:
            next_descriptor = os.open(component, flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = next_descriptor
        return descriptor
    except OSError:
        os.close(descriptor)
        raise


def open_library_audio(path_value: str, settings: Settings) -> tuple[int, int, str]:
    """Return an open descriptor, size, and MIME type for a permitted scanned file.

    The database path must still be canonical and inside a currently configured
    scan root. Directory traversal and the final open use no-follow descriptors,
    so replacing a path component with a symlink cannot redirect the read.
    """
    recorded_path = Path(path_value)
    if not recorded_path.is_absolute() or not settings.permitted_scan_roots:
        raise AudioFileAccessError("Audio file is outside permitted library roots")

    try:
        resolved_path = recorded_path.resolve(strict=True)
    except (OSError, RuntimeError) as exc:
        raise AudioFileAccessError("Audio file is unavailable") from exc
    if resolved_path != recorded_path:
        raise AudioFileAccessError("Audio file path is no longer canonical")

    selected_root: Path | None = None
    relative_path: Path | None = None
    for configured_root in settings.permitted_scan_roots:
        try:
            root = configured_root.expanduser().resolve(strict=True)
            relative = resolved_path.relative_to(root)
        except (OSError, RuntimeError, ValueError):
            continue
        if root.is_dir() and relative.parts:
            selected_root = root
            relative_path = relative
            break
    if selected_root is None or relative_path is None:
        raise AudioFileAccessError("Audio file is outside permitted library roots")

    suffix = resolved_path.suffix.lower()
    if suffix not in SUPPORTED_FORMATS:
        raise AudioFileAccessError("Audio format is not supported")

    directory_fd: int | None = None
    file_fd: int | None = None
    try:
        directory_fd = _open_directory(selected_root)
        directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | getattr(os, "O_CLOEXEC", 0)
        for component in relative_path.parts[:-1]:
            next_fd = os.open(component, directory_flags, dir_fd=directory_fd)
            os.close(directory_fd)
            directory_fd = next_fd

        file_fd = os.open(
            relative_path.parts[-1],
            os.O_RDONLY | os.O_NOFOLLOW | getattr(os, "O_CLOEXEC", 0),
            dir_fd=directory_fd,
        )
        file_stat = os.fstat(file_fd)
        if not stat.S_ISREG(file_stat.st_mode):
            raise AudioFileAccessError("Library entry is not a regular file")
        result = (file_fd, file_stat.st_size, CONTENT_TYPES[suffix])
        file_fd = None
        return result
    except AudioFileAccessError:
        raise
    except OSError as exc:
        raise AudioFileAccessError("Audio file could not be opened safely") from exc
    finally:
        if file_fd is not None:
            os.close(file_fd)
        if directory_fd is not None:
            os.close(directory_fd)


def read_audio_range(descriptor: int, start: int, length: int, chunk_size: int = 64 * 1024) -> Iterator[bytes]:
    """Stream a bounded range and always close its already-validated descriptor."""
    try:
        os.lseek(descriptor, start, os.SEEK_SET)
        remaining = length
        while remaining:
            chunk = os.read(descriptor, min(chunk_size, remaining))
            if not chunk:
                break
            remaining -= len(chunk)
            yield chunk
    finally:
        os.close(descriptor)
