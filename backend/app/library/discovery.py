"""Read-only, bounded discovery of supported audio files in a folder tree."""

from __future__ import annotations

from dataclasses import asdict, dataclass
import os
from os import scandir as _scandir
from pathlib import Path
import stat
from typing import Iterator

from .metadata import SUPPORTED_FORMATS


DEFAULT_MAX_ENTRIES = 100_000
DEFAULT_MAX_PREVIEW_FILES = 500
DEFAULT_MAX_ISSUE_SAMPLES = 100


def _entry_stat(entry: os.DirEntry[str]) -> os.stat_result:
    return entry.stat(follow_symlinks=False)


class DiscoveryPathError(ValueError):
    """Raised when the selected discovery root is not a safe directory."""


@dataclass(frozen=True)
class DiscoveredAudioFile:
    path: str
    size_bytes: int


@dataclass(frozen=True)
class DiscoveryIssue:
    path: str
    reason: str


@dataclass
class DiscoveryPreview:
    root: str
    supported_file_count: int
    supported_files: list[DiscoveredAudioFile]
    skipped_file_count: int
    skipped_files: list[DiscoveryIssue]
    error_count: int
    errors: list[DiscoveryIssue]
    entries_examined: int
    complete: bool
    limit_reached: bool
    file_list_truncated: bool

    def as_dict(self) -> dict:
        """Return a JSON-friendly bounded preview summary."""
        return asdict(self)


def _safe_root(directory: str | Path) -> Path:
    supplied_text = str(directory).strip()
    if not supplied_text:
        raise DiscoveryPathError("Folder is required")

    supplied = Path(directory).expanduser()
    try:
        if supplied.is_symlink():
            raise DiscoveryPathError("Selected folder cannot be a symbolic link")
        root = supplied.resolve(strict=True)
    except DiscoveryPathError:
        raise
    except (OSError, RuntimeError) as exc:
        raise DiscoveryPathError("Selected folder does not exist or cannot be resolved") from exc

    try:
        mode = root.stat().st_mode
    except OSError as exc:
        raise DiscoveryPathError("Selected folder cannot be accessed") from exc
    if not stat.S_ISDIR(mode):
        raise DiscoveryPathError("Selected path must be a directory")
    return root


def _relative_path(path: Path, root: Path) -> str:
    try:
        return path.relative_to(root).as_posix() or "."
    except ValueError:
        return path.name


def preview_audio_folder(
    directory: str | Path,
    *,
    max_entries: int = DEFAULT_MAX_ENTRIES,
    max_preview_files: int = DEFAULT_MAX_PREVIEW_FILES,
    max_issue_samples: int = DEFAULT_MAX_ISSUE_SAMPLES,
) -> DiscoveryPreview:
    """Discover common audio formats without reading, hashing, or writing files.

    Work is bounded by ``max_entries``. The returned path and issue lists are
    capped independently, while aggregate counts describe every examined entry.
    ``complete`` is false when a traversal or filesystem error prevents a full
    preview, or when the entry limit stops the walk.
    """
    if max_entries < 1 or max_preview_files < 0 or max_issue_samples < 0:
        raise ValueError("Discovery limits must be non-negative and max_entries must be positive")

    root = _safe_root(directory)
    supported_files: list[DiscoveredAudioFile] = []
    skipped_files: list[DiscoveryIssue] = []
    errors: list[DiscoveryIssue] = []
    supported_count = skipped_count = error_count = entries_examined = 0
    complete = True
    limit_reached = False
    file_list_truncated = False

    # A stack of open scandir iterators keeps traversal memory proportional to
    # tree depth rather than the number of directories in a wide folder.
    stack: list[tuple[Path, Iterator[os.DirEntry[str]]]] = []

    def record_error(path: Path, reason: str) -> None:
        nonlocal error_count, complete
        error_count += 1
        complete = False
        if len(errors) < max_issue_samples:
            errors.append(DiscoveryIssue(_relative_path(path, root), reason))

    def record_skipped(path: Path, reason: str) -> None:
        nonlocal skipped_count
        skipped_count += 1
        if len(skipped_files) < max_issue_samples:
            skipped_files.append(DiscoveryIssue(_relative_path(path, root), reason))

    try:
        try:
            stack.append((root, _scandir(root)))
        except OSError as exc:
            record_error(root, f"cannot read selected folder: {exc.strerror or exc.__class__.__name__}")

        while stack:
            current, entries = stack[-1]
            try:
                entry = next(entries)
            except StopIteration:
                close = getattr(entries, "close", None)
                if close:
                    close()
                stack.pop()
                continue
            except OSError as exc:
                record_error(current, f"cannot read folder entries: {exc.strerror or exc.__class__.__name__}")
                close = getattr(entries, "close", None)
                if close:
                    close()
                stack.pop()
                continue

            if entries_examined >= max_entries:
                complete = False
                limit_reached = True
                break
            entries_examined += 1
            path = Path(entry.path)

            try:
                if entry.is_symlink():
                    record_skipped(path, "symbolic link")
                    continue

                if entry.is_dir(follow_symlinks=False):
                    resolved = path.resolve(strict=True)
                    resolved.relative_to(root)
                    try:
                        child_entries = _scandir(resolved)
                    except OSError as exc:
                        record_error(path, f"cannot read folder: {exc.strerror or exc.__class__.__name__}")
                        continue
                    stack.append((resolved, child_entries))
                    continue

                entry_stat = _entry_stat(entry)
                if not stat.S_ISREG(entry_stat.st_mode):
                    record_skipped(path, "not a regular file")
                    continue

                resolved = path.resolve(strict=True)
                resolved.relative_to(root)
                if path.suffix.lower() not in SUPPORTED_FORMATS:
                    record_skipped(path, "unsupported file extension")
                    continue

                supported_count += 1
                if len(supported_files) < max_preview_files:
                    supported_files.append(
                        DiscoveredAudioFile(
                            path=_relative_path(resolved, root),
                            size_bytes=entry_stat.st_size,
                        )
                    )
                else:
                    file_list_truncated = True
            except ValueError:
                # A resolved child outside the selected root is never traversed
                # or returned, even if the filesystem changed during discovery.
                record_skipped(path, "path resolves outside selected folder")
            except OSError as exc:
                record_error(path, f"cannot inspect entry: {exc.strerror or exc.__class__.__name__}")
            except RuntimeError as exc:
                record_error(path, f"cannot resolve entry: {exc}")
    finally:
        for _, entries in reversed(stack):
            close = getattr(entries, "close", None)
            if close:
                close()

    return DiscoveryPreview(
        root=str(root),
        supported_file_count=supported_count,
        supported_files=supported_files,
        skipped_file_count=skipped_count,
        skipped_files=skipped_files,
        error_count=error_count,
        errors=errors,
        entries_examined=entries_examined,
        complete=complete,
        limit_reached=limit_reached,
        file_list_truncated=file_list_truncated,
    )
