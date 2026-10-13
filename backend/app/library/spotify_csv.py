"""Read-only parser for the currently supported Spotify library CSV shape."""

from __future__ import annotations

import csv
from collections import Counter, defaultdict
from functools import wraps
import io
from threading import RLock
from typing import Any


MAX_FILE_BYTES = 10 * 1024 * 1024
MAX_DATA_RECORDS = 20_000
EXPECTED_COLUMNS = (
    "Track name",
    "Artist name",
    "Album",
    "Playlist name",
    "Type",
    "ISRC",
    "Spotify - id",
)
SUPPORTED_TYPES = frozenset({"Album", "Artist", "Favorite", "Playlist"})
MAPPED_FIELDS = {
    "Track name": "track_name",
    "Artist name": "artist_name",
    "Album": "album",
    "Playlist name": "playlist_name",
    "Type": "source_type",
    "ISRC": "isrc",
    "Spotify - id": "spotify_id",
}
_CSV_FIELD_LIMIT_LOCK = RLock()


def _allow_bounded_csv_fields(function):
    """Raise csv's process-wide parser limit only during a locked preview call."""
    @wraps(function)
    def wrapped(*args, **kwargs):
        with _CSV_FIELD_LIMIT_LOCK:
            previous_limit = csv.field_size_limit()
            csv.field_size_limit(MAX_FILE_BYTES)
            try:
                return function(*args, **kwargs)
            finally:
                csv.field_size_limit(previous_limit)

    return wrapped


def _empty_result(
    file_errors: list[dict[str, Any]], *, ignored: int = 0, source_records: int = 0
) -> dict[str, Any]:
    return {
        "complete": False,
        "columns": [],
        "summary": {
            "source_records": source_records,
            "preview_rows": 0,
            "ignored_empty_records": ignored,
            "rows_with_errors": 0,
            "rows_with_warnings": 0,
        },
        "file_errors": file_errors,
        "rows": [],
        "duplicate_reports": [],
    }


def _is_empty_record(values: list[str]) -> bool:
    return all(not value.strip() for value in values)


def _header_errors(columns: list[str]) -> list[dict[str, Any]]:
    errors: list[dict[str, Any]] = []
    duplicates = sorted(name for name, count in Counter(columns).items() if count > 1)
    if duplicates:
        errors.append({"code": "duplicate_headers", "columns": duplicates})
    missing = [name for name in EXPECTED_COLUMNS if name not in columns]
    if missing:
        errors.append({"code": "missing_headers", "columns": missing})
    unexpected = [name for name in columns if name not in EXPECTED_COLUMNS]
    if unexpected:
        errors.append({"code": "unexpected_headers", "columns": unexpected})
    if not missing and not unexpected and columns != list(EXPECTED_COLUMNS):
        errors.append({"code": "header_order_mismatch"})
    return errors


def _mapped_row(columns: list[str], values: list[str]) -> dict[str, str | None]:
    mapped: dict[str, str | None] = {}
    for index, column in enumerate(columns):
        target = MAPPED_FIELDS.get(column)
        if target is not None and target not in mapped:
            mapped[target] = values[index] if index < len(values) else None
    for target in MAPPED_FIELDS.values():
        mapped.setdefault(target, None)
    return mapped


def _duplicate_reports(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    exact: dict[tuple[str, ...], list[int]] = defaultdict(list)
    by_id: dict[str, list[dict[str, Any]]] = defaultdict(list)
    by_id_playlist: dict[tuple[str, str], list[int]] = defaultdict(list)

    for row in rows:
        values = tuple(row["source_values"])
        exact[values].append(row["source_record"])
        mapped = row["mapped"]
        spotify_id = (mapped["spotify_id"] or "").strip()
        playlist = (mapped["playlist_name"] or "").strip()
        if spotify_id:
            by_id[spotify_id].append(row)
            if playlist:
                by_id_playlist[(spotify_id, playlist)].append(row["source_record"])

    reports: list[dict[str, Any]] = []

    def add(category: str, records: list[int], differing_fields: list[str] | None = None) -> None:
        report: dict[str, Any] = {"category": category, "source_records": sorted(records)}
        if differing_fields is not None:
            report["differing_fields"] = differing_fields
        reports.append(report)

    for records in exact.values():
        if len(records) > 1:
            add("exact_row_duplicate", records)
    for records in by_id_playlist.values():
        if len(records) > 1:
            add("repeated_id_within_playlist", records)
    for occurrences in by_id.values():
        playlist_labels = {
            (row["mapped"]["playlist_name"] or "").strip()
            for row in occurrences
            if (row["mapped"]["playlist_name"] or "").strip()
        }
        if len(playlist_labels) > 1:
            add("id_across_playlists", [row["source_record"] for row in occurrences])

        compared = ("track_name", "artist_name", "album", "source_type")
        differing = [
            field
            for field in compared
            if len({row["mapped"][field] for row in occurrences}) > 1
        ]
        if len(occurrences) > 1 and differing:
            add("metadata_conflict", [row["source_record"] for row in occurrences], differing)

    reports.sort(key=lambda report: (report["source_records"], report["category"]))
    for index, report in enumerate(reports, 1):
        report["report_id"] = f"duplicate-{index}"
    return reports


def _finish(
    columns: list[str],
    rows: list[dict[str, Any]],
    file_errors: list[dict[str, Any]],
    ignored: int,
    complete: bool,
    source_records: int,
) -> dict[str, Any]:
    reports = _duplicate_reports(rows)
    report_by_record: dict[int, list[str]] = defaultdict(list)
    for report in reports:
        for record in report["source_records"]:
            report_by_record[record].append(report["report_id"])
    for row in rows:
        row["duplicate_reports"] = report_by_record[row["source_record"]]

    return {
        "complete": complete,
        "columns": columns,
        "summary": {
            "source_records": source_records,
            "preview_rows": len(rows),
            "ignored_empty_records": ignored,
            "rows_with_errors": sum(bool(row["errors"]) for row in rows),
            "rows_with_warnings": sum(bool(row["warnings"]) for row in rows),
        },
        "file_errors": file_errors,
        "rows": rows,
        "duplicate_reports": reports,
    }


@_allow_bounded_csv_fields
def preview_spotify_csv(content: bytes | bytearray) -> dict[str, Any]:
    """Parse a bounded CSV upload without touching persistence or the filesystem."""
    if len(content) > MAX_FILE_BYTES:
        return _empty_result([
            {"code": "file_too_large", "max_bytes": MAX_FILE_BYTES, "actual_bytes": len(content)}
        ])

    try:
        text = content.decode("utf-8-sig", errors="strict")
    except UnicodeDecodeError as exc:
        return _empty_result([
            {"code": "invalid_encoding", "byte_start": exc.start, "byte_end": exc.end}
        ])

    stream = io.StringIO(text, newline="")
    reader = csv.reader(stream, strict=True)
    rows: list[dict[str, Any]] = []
    columns: list[str] = []
    file_errors: list[dict[str, Any]] = []
    ignored = 0
    source_records = 0
    record_ordinal = 0
    previous_line = 0
    header_read = False

    try:
        raw_header = next(reader)
        header_read = True
        columns = raw_header
        file_errors.extend(_header_errors(columns))
        previous_line = reader.line_num
    except StopIteration:
        return _finish([], [], [{"code": "missing_header"}], 0, True, 0)
    except csv.Error as exc:
        return _finish([], [], [{
            "code": "csv_parse_error",
            "message": str(exc),
            "physical_line_start": 1,
            "physical_line_end": max(reader.line_num, 1),
        }], 0, False, 0)

    try:
        while True:
            line_start = previous_line + 1
            try:
                values = next(reader)
            except StopIteration:
                break
            except csv.Error as exc:
                file_errors.append({
                    "code": "csv_parse_error",
                    "message": str(exc),
                    "physical_line_start": line_start,
                    "physical_line_end": max(reader.line_num, line_start),
                })
                return _finish(columns, rows, file_errors, ignored, False, source_records)

            line_end = reader.line_num
            previous_line = line_end
            record_ordinal += 1
            if _is_empty_record(values):
                ignored += 1
                continue

            source_records += 1
            if source_records > MAX_DATA_RECORDS:
                return _empty_result(file_errors + [{
                    "code": "row_limit_exceeded",
                    "max_records": MAX_DATA_RECORDS,
                    "observed_records": source_records,
                }], ignored=ignored, source_records=source_records)

            errors: list[dict[str, Any]] = []
            warnings: list[dict[str, Any]] = []
            if len(values) != len(EXPECTED_COLUMNS):
                errors.append({
                    "code": "field_count_mismatch",
                    "expected": len(EXPECTED_COLUMNS),
                    "actual": len(values),
                })
            mapped = _mapped_row(columns, values)
            source_type = mapped["source_type"]
            if source_type is None or not source_type.strip():
                errors.append({"code": "missing_type"})
            elif source_type not in SUPPORTED_TYPES:
                warnings.append({"code": "unsupported_type", "value": source_type})
            for field in ("track_name", "artist_name", "album"):
                value = mapped[field]
                if value is None or not value.strip():
                    warnings.append({"code": "blank_descriptive_field", "field": field})
            rows.append({
                "source_record": record_ordinal,
                "physical_line_start": line_start,
                "physical_line_end": line_end,
                "source_values": values,
                "mapped": mapped,
                "errors": errors,
                "warnings": warnings,
                "duplicate_reports": [],
            })
    except csv.Error as exc:
        # Defensive fallback for parser errors raised outside next(reader).
        file_errors.append({"code": "csv_parse_error", "message": str(exc)})
        return _finish(columns, rows, file_errors, ignored, False, source_records)

    return _finish(columns, rows, file_errors, ignored, True, source_records)
