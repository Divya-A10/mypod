"""Preview-only CSV endpoint. This module intentionally has no database imports."""

from fastapi import APIRouter, HTTPException, Request

from ..library.spotify_csv import MAX_FILE_BYTES, preview_spotify_csv


router = APIRouter()


@router.post("/imports/spotify/preview")
async def preview_spotify_import(request: Request) -> dict:
    """Stream a bounded CSV body, then validate it in memory without persistence."""
    content_length = request.headers.get("content-length")
    if content_length is not None:
        try:
            declared_length = int(content_length)
        except ValueError:
            declared_length = None
        if declared_length is not None and declared_length > MAX_FILE_BYTES:
            raise HTTPException(
                status_code=413,
                detail={"code": "file_too_large", "max_bytes": MAX_FILE_BYTES},
            )

    content = bytearray()
    async for chunk in request.stream():
        if len(chunk) > MAX_FILE_BYTES - len(content):
            raise HTTPException(
                status_code=413,
                detail={"code": "file_too_large", "max_bytes": MAX_FILE_BYTES},
            )
        content.extend(chunk)

    return preview_spotify_csv(content)
