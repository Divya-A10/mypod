import asyncio
import csv
import io

import httpx
from fastapi import HTTPException
from starlette.requests import Request

from backend.app import database
from backend.app.api.spotify_preview import preview_spotify_import
from backend.app.library.spotify_csv import (
    EXPECTED_COLUMNS,
    MAX_DATA_RECORDS,
    MAX_FILE_BYTES,
    preview_spotify_csv,
)
from backend.app.main import app


def csv_bytes(rows, *, headers=EXPECTED_COLUMNS, bom=False):
    output = io.StringIO(newline="")
    writer = csv.writer(output, lineterminator="\r\n")
    writer.writerow(headers)
    writer.writerows(rows)
    encoded = output.getvalue().encode("utf-8")
    return (b"\xef\xbb\xbf" + encoded) if bom else encoded


def valid_row(name="Synthetic track", artist="Synthetic artist", album="Synthetic album", playlist="Synthetic playlist", kind="Playlist", isrc="TEST00000001", spotify_id="synthetic-id"):
    return [name, artist, album, playlist, kind, isrc, spotify_id]


def test_preview_handles_quoted_commas_and_multiline_fields():
    content = csv_bytes([valid_row('Synthetic, "quoted" title', "Artist\nwith line break")])

    result = preview_spotify_csv(content)

    assert result["complete"] is True
    row = result["rows"][0]
    assert row["mapped"]["track_name"] == 'Synthetic, "quoted" title'
    assert row["mapped"]["artist_name"] == "Artist\nwith line break"
    assert (row["physical_line_start"], row["physical_line_end"]) == (2, 3)


def test_preview_accepts_bom_and_reports_invalid_utf8():
    assert preview_spotify_csv(csv_bytes([valid_row()], bom=True))["complete"] is True

    result = preview_spotify_csv(b"\xff\xfe\xfa")

    assert result["complete"] is False
    assert result["file_errors"][0]["code"] == "invalid_encoding"


def test_preview_reports_header_problems_and_preserves_parseable_rows():
    duplicate_headers = list(EXPECTED_COLUMNS[:-1]) + ["Track name"]
    duplicate = preview_spotify_csv(csv_bytes([valid_row()], headers=duplicate_headers))
    assert any(error["code"] == "duplicate_headers" for error in duplicate["file_errors"])
    assert any(error["code"] == "missing_headers" for error in duplicate["file_errors"])
    assert duplicate["summary"]["preview_rows"] == 1

    missing = preview_spotify_csv(csv_bytes([["x"]], headers=EXPECTED_COLUMNS[:-1]))
    assert any(error["code"] == "missing_headers" for error in missing["file_errors"])
    assert missing["rows"][0]["errors"][0]["code"] == "field_count_mismatch"

    reordered_columns = list(EXPECTED_COLUMNS)
    reordered_columns[0], reordered_columns[1] = reordered_columns[1], reordered_columns[0]
    reordered = preview_spotify_csv(csv_bytes([valid_row()], headers=reordered_columns))
    assert any(error["code"] == "header_order_mismatch" for error in reordered["file_errors"])

    unexpected_columns = list(EXPECTED_COLUMNS) + ["Unexpected"]
    unexpected_row = valid_row() + ["synthetic extra value"]
    unexpected = preview_spotify_csv(csv_bytes([unexpected_row], headers=unexpected_columns))
    assert any(error["code"] == "unexpected_headers" for error in unexpected["file_errors"])
    assert not any(error["code"] == "missing_headers" for error in unexpected["file_errors"])


def test_preview_ignores_empty_records_and_keeps_malformed_width_rows():
    content = csv_bytes([[], [" ", "", "", "", "", "", ""], ["short", "row"]])

    result = preview_spotify_csv(content)

    assert result["complete"] is True
    assert result["summary"]["ignored_empty_records"] == 2
    assert result["summary"]["preview_rows"] == 1
    assert result["rows"][0]["source_record"] == 3
    assert result["rows"][0]["errors"][0]["code"] == "field_count_mismatch"


def test_preview_allows_optional_identifiers_and_warns_on_unknown_type():
    row = valid_row(kind="FutureKind", isrc="", spotify_id="")

    result = preview_spotify_csv(csv_bytes([row]))

    assert result["complete"] is True
    assert result["rows"][0]["mapped"]["isrc"] == ""
    assert result["rows"][0]["mapped"]["spotify_id"] == ""
    assert {warning["code"] for warning in result["rows"][0]["warnings"]} == {"unsupported_type"}


def test_preview_reports_duplicate_categories_and_metadata_conflicts():
    rows = [
        valid_row(spotify_id="same-id", playlist="Synthetic A"),
        valid_row(spotify_id="same-id", playlist="Synthetic A"),
        valid_row(artist="Changed synthetic artist", spotify_id="same-id", playlist="Synthetic B"),
    ]

    result = preview_spotify_csv(csv_bytes(rows))

    reports = {report["category"]: report for report in result["duplicate_reports"]}
    assert reports["exact_row_duplicate"]["source_records"] == [1, 2]
    assert reports["repeated_id_within_playlist"]["source_records"] == [1, 2]
    assert reports["id_across_playlists"]["source_records"] == [1, 2, 3]
    assert reports["metadata_conflict"]["differing_fields"] == ["artist_name"]
    assert len(reports["metadata_conflict"]["source_records"]) == 3
    assert all(row["duplicate_reports"] for row in result["rows"])


def test_preview_fatal_parse_error_is_explicitly_incomplete():
    content = csv_bytes([valid_row()]) + b'"unterminated field\r\n'

    result = preview_spotify_csv(content)

    assert result["complete"] is False
    assert result["summary"]["preview_rows"] == 1
    assert result["file_errors"][-1]["code"] == "csv_parse_error"
    assert "physical_line_start" in result["file_errors"][-1]
    assert "physical_line_end" in result["file_errors"][-1]


def test_preview_accepts_exact_file_and_record_limits_and_rejects_one_over():
    large_field = csv_bytes([["x" * 200_000, "Synthetic artist", "Synthetic album", "Synthetic playlist", "Playlist", "", "id"]])
    assert preview_spotify_csv(large_field)["complete"] is True

    base = csv_bytes([valid_row()])
    exact_file = base
    remainder = MAX_FILE_BYTES - len(exact_file)
    filler_tail = ["Synthetic artist", "Synthetic album", "Synthetic playlist", "Playlist", "", ""]
    empty_name_row_size = len(csv_bytes([[""] + filler_tail])) - len(csv_bytes([]))
    while remainder > empty_name_row_size + 120_000:
        line = csv_bytes([["x" * 120_000] + filler_tail])
        line = line[len(csv_bytes([])):]
        exact_file += line
        remainder -= len(line)
    if remainder >= empty_name_row_size:
        name_length = remainder - empty_name_row_size
        line = csv_bytes([["x" * name_length] + filler_tail])
        line = line[len(csv_bytes([])):]
        exact_file += line
        remainder -= len(line)
    exact_file += b"\r\n" * (remainder // 2)
    if remainder % 2:
        exact_file += b" "
    assert len(exact_file) == MAX_FILE_BYTES
    assert preview_spotify_csv(exact_file)["complete"] is True
    over_file = preview_spotify_csv(exact_file + b" ")
    assert over_file["complete"] is False
    assert over_file["file_errors"][0]["code"] == "file_too_large"
    assert over_file["rows"] == []

    exact_records = [valid_row(spotify_id=f"synthetic-{index}") for index in range(MAX_DATA_RECORDS)]
    exact_rows_result = preview_spotify_csv(csv_bytes(exact_records))
    assert exact_rows_result["complete"] is True
    assert exact_rows_result["summary"]["preview_rows"] == MAX_DATA_RECORDS

    over_records = exact_records + [valid_row(spotify_id="synthetic-over-limit")]
    over_rows_result = preview_spotify_csv(csv_bytes(over_records))
    assert over_rows_result["complete"] is False
    assert over_rows_result["file_errors"][-1]["code"] == "row_limit_exceeded"
    assert over_rows_result["rows"] == []


def test_preview_endpoint_never_calls_database(monkeypatch):
    def fail_if_called(*args, **kwargs):
        raise AssertionError("preview attempted database access")

    monkeypatch.setattr(database, "connect", fail_if_called)

    async def exercise():
        transport = httpx.ASGITransport(app=app)
        async with httpx.AsyncClient(transport=transport, base_url="http://test") as client:
            return await client.post(
                "/imports/spotify/preview",
                content=csv_bytes([valid_row(), ["short", "synthetic malformed row"]]),
                headers={"content-type": "text/csv"},
            )

    response = asyncio.run(exercise())

    assert response.status_code == 200
    assert response.json()["complete"] is True
    assert len(response.json()["rows"]) == 2


def test_preview_endpoint_rejects_oversized_stream_without_consuming_the_rest():
    chunk_size = 1024 * 1024
    chunks = [b"x" * chunk_size for _ in range(12)]
    received = 0

    async def exercise():
        nonlocal received

        async def receive():
            nonlocal received
            chunk = chunks[received]
            received += 1
            return {
                "type": "http.request",
                "body": chunk,
                "more_body": received < len(chunks),
            }

        request = Request(
            {
                "type": "http",
                "method": "POST",
                "path": "/imports/spotify/preview",
                "headers": [(b"content-type", b"text/csv")],
            },
            receive,
        )
        try:
            await preview_spotify_import(request)
        except HTTPException as exc:
            return exc.status_code, exc.detail
        raise AssertionError("oversized body should be rejected")

    status, detail = asyncio.run(exercise())

    assert status == 413
    assert detail == {"code": "file_too_large", "max_bytes": MAX_FILE_BYTES}
    assert received == 11
    assert received < len(chunks)
