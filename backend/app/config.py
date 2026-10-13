"""Environment-based settings for the local myPod service."""

from dataclasses import dataclass
import os
from pathlib import Path


@dataclass(frozen=True)
class Settings:
    database_path: Path
    managed_music_dir: Path
    permitted_scan_roots: tuple[Path, ...]

    @classmethod
    def from_env(cls) -> "Settings":
        data_dir = Path(os.environ.get("MYPOD_DATA_DIR", Path.home() / ".localpod"))
        root_values = os.environ.get("MYPOD_SCAN_ROOTS", "")
        roots = tuple(Path(value).expanduser() for value in root_values.split(os.pathsep) if value)
        return cls(
            database_path=Path(os.environ.get("MYPOD_DATABASE_PATH", data_dir / "library.sqlite3")).expanduser(),
            managed_music_dir=Path(os.environ.get("MYPOD_MUSIC_DIR", data_dir / "music")).expanduser(),
            permitted_scan_roots=roots,
        )
