from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from .api.library import router as library_router
from .api.spotify_preview import router as spotify_preview_router

app = FastAPI(title="myPod API")
app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "http://localhost:1420",
        "http://127.0.0.1:1420",
        "tauri://localhost",
        "http://tauri.localhost",
        "https://tauri.localhost",
    ],
    allow_methods=["GET"],
    allow_headers=[],
)
app.include_router(library_router)
app.include_router(spotify_preview_router)


@app.get("/health")
def health_check() -> dict[str, str]:
    return {"status": "ok"}
