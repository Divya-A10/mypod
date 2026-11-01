import os

import pytest

from backend.app.library import discovery
from backend.app.library.discovery import DiscoveryPathError, preview_audio_folder


def test_discovers_supported_formats_recursively_without_changing_files(tmp_path):
    root = tmp_path / "music"
    nested = root / "Live" / "Set 1"
    nested.mkdir(parents=True)
    originals = {
        root / "one.MP3": b"synthetic mp3 fixture",
        root / "two.flac": b"synthetic flac fixture",
        nested / "three.m4a": b"synthetic m4a fixture",
        nested / "four.ogg": b"synthetic ogg fixture",
        nested / "five.wav": b"synthetic wav fixture",
    }
    for path, content in originals.items():
        path.write_bytes(content)

    preview = preview_audio_folder(root)

    assert preview.complete is True
    assert preview.supported_file_count == 5
    assert {item.path for item in preview.supported_files} == {
        "one.MP3", "two.flac", "Live/Set 1/three.m4a", "Live/Set 1/four.ogg", "Live/Set 1/five.wav",
    }
    assert {item.size_bytes for item in preview.supported_files} == {len(value) for value in originals.values()}
    assert all(path.read_bytes() == content for path, content in originals.items())


def test_skips_symlinks_and_unsupported_files_without_leaving_selected_root(tmp_path):
    root = tmp_path / "music"
    root.mkdir()
    outside = tmp_path / "outside"
    outside.mkdir()
    (root / "inside.wav").write_bytes(b"inside")
    (outside / "outside.mp3").write_bytes(b"outside")
    (root / "notes.txt").write_text("not audio")
    (root / "linked-file.wav").symlink_to(outside / "outside.mp3")
    (root / "linked-folder").symlink_to(outside, target_is_directory=True)

    preview = preview_audio_folder(root)

    assert preview.supported_file_count == 1
    assert [item.path for item in preview.supported_files] == ["inside.wav"]
    assert preview.skipped_file_count == 3
    assert {item.reason for item in preview.skipped_files} == {"symbolic link", "unsupported file extension"}
    assert preview.error_count == 0


def test_skips_non_regular_files(tmp_path):
    if not hasattr(os, "mkfifo"):
        pytest.skip("FIFO fixtures are unavailable on this platform")
    root = tmp_path / "music"
    root.mkdir()
    os.mkfifo(root / "pipe.wav")
    (root / "song.wav").write_bytes(b"audio")

    preview = preview_audio_folder(root)

    assert preview.supported_file_count == 1
    assert preview.skipped_file_count == 1
    assert preview.skipped_files[0].reason == "not a regular file"


@pytest.mark.parametrize("make_root", ["symlink", "file", "missing", "empty"])
def test_rejects_invalid_selected_roots(tmp_path, make_root):
    target = tmp_path / "target"
    target.mkdir()
    if make_root == "symlink":
        selected = tmp_path / "alias"
        selected.symlink_to(target, target_is_directory=True)
    elif make_root == "file":
        selected = tmp_path / "audio.wav"
        selected.write_bytes(b"audio")
    elif make_root == "missing":
        selected = tmp_path / "missing"
    else:
        selected = "   "

    with pytest.raises(DiscoveryPathError):
        preview_audio_folder(selected)


def test_inaccessible_file_is_reported_and_later_files_are_still_discovered(tmp_path, monkeypatch):
    root = tmp_path / "music"
    root.mkdir()
    denied = root / "denied.wav"
    denied.write_bytes(b"unreadable synthetic entry")
    (root / "readable.wav").write_bytes(b"readable")
    real_stat = discovery._entry_stat

    def stat_with_denial(entry):
        if entry.name == denied.name:
            raise PermissionError("synthetic permission denial")
        return real_stat(entry)

    monkeypatch.setattr(discovery, "_entry_stat", stat_with_denial)

    preview = preview_audio_folder(root)

    assert preview.complete is False
    assert preview.error_count == 1
    assert preview.errors[0].path == denied.name
    assert preview.supported_file_count == 1
    assert preview.supported_files[0].path == "readable.wav"


def test_inaccessible_subfolder_is_reported_without_aborting_other_branches(tmp_path, monkeypatch):
    root = tmp_path / "music"
    blocked = root / "blocked"
    open_folder = root / "available"
    blocked.mkdir(parents=True)
    open_folder.mkdir()
    (blocked / "hidden.wav").write_bytes(b"not read")
    (open_folder / "visible.wav").write_bytes(b"read")
    real_scandir = discovery._scandir

    def scandir_with_denial(path):
        if os.fspath(path) == os.fspath(blocked):
            raise PermissionError("synthetic directory denial")
        return real_scandir(path)

    monkeypatch.setattr(discovery, "_scandir", scandir_with_denial)

    preview = preview_audio_folder(root)

    assert preview.complete is False
    assert preview.error_count == 1
    assert preview.errors[0].path == "blocked"
    assert [item.path for item in preview.supported_files] == ["available/visible.wav"]


def test_preview_lists_and_traversal_work_are_bounded_and_report_truncation(tmp_path):
    root = tmp_path / "music"
    root.mkdir()
    for index in range(8):
        (root / f"{index}.wav").write_bytes(b"tone")
    (root / "note.txt").write_text("skip")

    preview = preview_audio_folder(root, max_entries=5, max_preview_files=2, max_issue_samples=1)

    assert preview.entries_examined == 5
    assert preview.complete is False
    assert preview.limit_reached is True
    assert len(preview.supported_files) <= 2
    assert len(preview.skipped_files) <= 1
    assert preview.supported_file_count + preview.skipped_file_count <= preview.entries_examined


def test_exact_entry_limit_can_be_complete_and_preview_file_list_is_bounded(tmp_path):
    root = tmp_path / "music"
    root.mkdir()
    (root / "one.wav").write_bytes(b"one")
    (root / "two.wav").write_bytes(b"two")

    preview = preview_audio_folder(root, max_entries=2, max_preview_files=1)

    assert preview.entries_examined == 2
    assert preview.complete is True
    assert preview.limit_reached is False
    assert preview.supported_file_count == 2
    assert len(preview.supported_files) == 1
    assert preview.file_list_truncated is True


def test_empty_directory_returns_complete_empty_preview(tmp_path):
    root = tmp_path / "empty"
    root.mkdir()

    preview = preview_audio_folder(root)

    assert preview.complete is True
    assert preview.supported_file_count == preview.skipped_file_count == preview.error_count == 0
    assert preview.as_dict()["supported_files"] == []
