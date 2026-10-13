"""Library and explicit scan API endpoints."""

import re
import os

from fastapi import APIRouter, Depends, Header, HTTPException, Query
from pydantic import BaseModel
from starlette.responses import Response, StreamingResponse

from ..config import Settings
from ..database import get_track, list_tracks
from ..library.audio import AudioFileAccessError, open_library_audio, read_audio_range
from ..library.scanner import ScanPathError, scan_directory


router = APIRouter()


class ScanRequest(BaseModel):
    directory: str


def get_settings() -> Settings:
    return Settings.from_env()


@router.get("/tracks")
def tracks(
    limit: int = Query(default=50, ge=1, le=100),
    offset: int = Query(default=0, ge=0),
    settings: Settings = Depends(get_settings),
) -> dict:
    items, total = list_tracks(settings.database_path, limit, offset)
    return {"items": items, "total": total, "limit": limit, "offset": offset}


@router.get("/tracks/{track_id}")
def track(track_id: int, settings: Settings = Depends(get_settings)) -> dict:
    item = get_track(settings.database_path, track_id)
    if item is None:
        raise HTTPException(status_code=404, detail="Track not found")
    return item


def _parse_byte_range(value: str, size: int) -> tuple[int, int] | None:
    """Parse one RFC-style byte range; return None for malformed/multi ranges."""
    match = re.fullmatch(r"bytes=(\d*)-(\d*)", value.strip())
    if match is None or not (match.group(1) or match.group(2)):
        return None
    if size <= 0:
        raise ValueError("unsatisfiable")

    first, last = match.groups()
    if first:
        start = int(first)
        end = min(int(last), size - 1) if last else size - 1
        if start >= size or start > end:
            raise ValueError("unsatisfiable")
        return start, end

    suffix_length = int(last)
    if suffix_length <= 0:
        raise ValueError("unsatisfiable")
    return max(0, size - suffix_length), size - 1


@router.get("/tracks/{track_id}/audio")
def track_audio(
    track_id: int,
    range_header: str | None = Header(default=None, alias="Range"),
    settings: Settings = Depends(get_settings),
):
    """Stream an indexed track by ID, never by a caller-supplied file path."""
    if track_id < 1:
        raise HTTPException(status_code=404, detail="Track not found")
    item = get_track(settings.database_path, track_id)
    if item is None or item["is_missing"]:
        raise HTTPException(status_code=404, detail="Track audio is unavailable")

    try:
        descriptor, size, content_type = open_library_audio(item["path"], settings)
    except AudioFileAccessError as exc:
        raise HTTPException(status_code=404, detail="Track audio is unavailable") from exc

    status_code = 200
    start, end = 0, size - 1
    headers = {
        "Accept-Ranges": "bytes",
        "Cache-Control": "no-store",
        "Content-Length": str(size),
        "Content-Type": content_type,
        "X-Content-Type-Options": "nosniff",
    }
    if range_header is not None:
        try:
            parsed_range = _parse_byte_range(range_header, size)
        except ValueError:
            os.close(descriptor)
            return Response(status_code=416, headers={
                "Accept-Ranges": "bytes",
                "Cache-Control": "no-store",
                "Content-Range": f"bytes */{size}",
                "Content-Length": "0",
            })
        if parsed_range is not None:
            start, end = parsed_range
            status_code = 206
            headers["Content-Length"] = str(end - start + 1)
            headers["Content-Range"] = f"bytes {start}-{end}/{size}"

    return StreamingResponse(
        read_audio_range(descriptor, start, end - start + 1),
        status_code=status_code,
        headers=headers,
    )


@router.post("/scan")
def scan(request: ScanRequest, settings: Settings = Depends(get_settings)) -> dict[str, int]:
    try:
        return scan_directory(request.directory, settings)
    except ScanPathError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
