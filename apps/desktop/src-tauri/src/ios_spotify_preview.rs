//! Read-only, bounded Spotify CSV preview for iOS.
//!
//! This module deliberately has no catalog/database imports. The only file it
//! opens is a short-lived, token-named file in MyPod's existing import staging
//! directory. The file is removed when the command returns, including errors.

use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
#[cfg(any(target_os = "ios", test))]
use std::fs::{self, File};
#[cfg(any(target_os = "ios", test))]
use std::io::Read;
#[cfg(any(target_os = "ios", test))]
use std::path::{Path, PathBuf};

#[cfg(target_os = "ios")]
use tauri::Manager;

#[cfg(any(target_os = "ios", test))]
const APP_FOLDER: &str = "MyPod";
#[cfg(any(target_os = "ios", test))]
const STAGING_FOLDER: &str = "Importing";
const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
const MAX_DATA_RECORDS: usize = 20_000;
const EXPECTED_COLUMNS: [&str; 7] = [
    "Track name",
    "Artist name",
    "Album",
    "Playlist name",
    "Type",
    "ISRC",
    "Spotify - id",
];

#[cfg(any(target_os = "ios", test))]
struct StagedCsvCleanup(PathBuf);

#[cfg(any(target_os = "ios", test))]
impl Drop for StagedCsvCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub(crate) struct CsvIssue {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_line_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_line_end: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_records: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_records: Option<usize>,
}

impl CsvIssue {
    fn code(code: &str) -> Self {
        Self {
            code: code.to_string(),
            ..Self::default()
        }
    }

    fn parse_error(message: String, start: usize, end: usize) -> Self {
        Self {
            code: "csv_parse_error".into(),
            message: Some(message),
            physical_line_start: Some(start),
            physical_line_end: Some(end),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub(crate) struct MappedFields {
    pub track_name: Option<String>,
    pub artist_name: Option<String>,
    pub album: Option<String>,
    pub playlist_name: Option<String>,
    pub source_type: Option<String>,
    pub isrc: Option<String>,
    pub spotify_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct SpotifyCsvRow {
    pub source_record: usize,
    pub physical_line_start: usize,
    pub physical_line_end: usize,
    pub source_values: Vec<String>,
    pub mapped: MappedFields,
    pub errors: Vec<CsvIssue>,
    pub warnings: Vec<CsvIssue>,
    pub duplicate_reports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub(crate) struct DuplicateReport {
    pub report_id: String,
    pub category: String,
    pub source_records: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub differing_fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub(crate) struct PreviewSummary {
    pub source_records: usize,
    pub preview_rows: usize,
    pub ignored_empty_records: usize,
    pub ignored_empty_spans: Vec<EmptyRecordSpan>,
    pub rows_with_errors: usize,
    pub rows_with_warnings: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct EmptyRecordSpan {
    pub source_record_start: usize,
    pub source_record_end: usize,
    pub physical_line_start: usize,
    pub physical_line_end: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub(crate) struct SpotifyCsvPreview {
    pub complete: bool,
    pub columns: Vec<String>,
    pub summary: PreviewSummary,
    pub file_errors: Vec<CsvIssue>,
    pub rows: Vec<SpotifyCsvRow>,
    pub duplicate_reports: Vec<DuplicateReport>,
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn preview_spotify_csv(
    app: tauri::AppHandle,
    import_id: String,
) -> Result<SpotifyCsvPreview, String> {
    if !valid_import_id(&import_id) {
        return Err("Invalid temporary preview token".to_string());
    }
    let app_data = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let id = import_id.clone();
    tauri::async_runtime::spawn_blocking(move || preview_staged_file(&app_data, &id))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(any(target_os = "ios", test))]
fn valid_import_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(any(target_os = "ios", test))]
fn preview_staged_file(app_data: &Path, import_id: &str) -> Result<SpotifyCsvPreview, String> {
    if !valid_import_id(import_id) {
        return Err("Invalid temporary preview token".to_string());
    }
    reject_symlink(app_data)?;
    let app_data = app_data.canonicalize().map_err(|error| error.to_string())?;
    let app_root = app_data.join(APP_FOLDER);
    let staging = app_root.join(STAGING_FOLDER);
    reject_symlink(&app_root)?;
    reject_symlink(&staging)?;
    let canonical_staging = staging.canonicalize().map_err(|error| error.to_string())?;
    if !canonical_staging.starts_with(&app_data) {
        return Err("Preview staging directory escaped app-private storage".into());
    }

    let path = canonical_staging.join(format!("spotify-{import_id}.csv.part"));
    let cleanup = StagedCsvCleanup(path.clone());
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("The selected CSV staging item is not a regular file".into());
    }
    let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if length > MAX_FILE_BYTES {
        return Ok(file_too_large(length));
    }

    // Read one byte beyond the limit so a file that changed after stat is
    // rejected rather than parsed as a truncated preview.
    let mut file = File::open(&path).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("The selected CSV staging item is not a regular file".into());
    }
    let mut bytes = Vec::with_capacity(length.min(MAX_FILE_BYTES));
    file.by_ref()
        .take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    drop(file);
    drop(cleanup);
    if bytes.len() > MAX_FILE_BYTES {
        return Ok(file_too_large_unknown());
    }
    Ok(parse_spotify_csv(&bytes))
}

#[cfg(any(target_os = "ios", test))]
fn reject_symlink(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("Symlink paths are not accepted for CSV preview".into());
    }
    Ok(())
}

fn file_too_large(actual: usize) -> SpotifyCsvPreview {
    let mut issue = CsvIssue::code("file_too_large");
    issue.max_bytes = Some(MAX_FILE_BYTES);
    issue.actual_bytes = Some(actual);
    SpotifyCsvPreview {
        file_errors: vec![issue],
        ..SpotifyCsvPreview::default()
    }
}

#[cfg(any(target_os = "ios", test))]
fn file_too_large_unknown() -> SpotifyCsvPreview {
    let mut issue = CsvIssue::code("file_too_large");
    issue.max_bytes = Some(MAX_FILE_BYTES);
    issue.message = Some("The staged file exceeds the preview limit".into());
    SpotifyCsvPreview {
        file_errors: vec![issue],
        ..SpotifyCsvPreview::default()
    }
}

#[derive(Debug)]
struct ParsedRecord {
    fields: Vec<String>,
    line_start: usize,
    line_end: usize,
    parse_issue: Option<CsvIssue>,
}

#[derive(Debug)]
struct CsvParseFailure {
    message: String,
    line_start: usize,
    line_end: usize,
    fields: Vec<String>,
    limit_exceeded: bool,
}

fn parse_failure(
    message: &str,
    line_start: usize,
    line_end: usize,
    mut fields: Vec<String>,
    field: String,
) -> CsvParseFailure {
    fields.push(field);
    CsvParseFailure {
        message: message.to_string(),
        line_start,
        line_end,
        fields,
        limit_exceeded: false,
    }
}

/// Strict RFC-4180-style parser that preserves physical line spans.
/// CRLF, LF and CR record terminators are accepted; quoted line breaks are data.
fn parse_records(input: &str) -> (Vec<ParsedRecord>, Option<CsvParseFailure>) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Start,
        Unquoted,
        Quoted,
        AfterQuote,
    }

    let chars: Vec<char> = input.chars().collect();
    let mut records = Vec::new();
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut state = State::Start;
    let mut index = 0;
    let mut line = 1;
    let mut record_start = 1;
    let mut record_has_input = false;

    while index < chars.len() {
        let ch = chars[index];
        let newline_len = if ch == '\r' && chars.get(index + 1) == Some(&'\n') {
            2
        } else {
            1
        };
        let is_newline = ch == '\r' || ch == '\n';
        match state {
            State::Start => match ch {
                '"' => {
                    state = State::Quoted;
                    record_has_input = true;
                    index += 1;
                }
                ',' => {
                    fields.push(String::new());
                    record_has_input = true;
                    index += 1;
                }
                '\r' | '\n' => {
                    fields.push(String::new());
                    records.push(ParsedRecord {
                        fields: std::mem::take(&mut fields),
                        line_start: record_start,
                        line_end: line,
                        parse_issue: None,
                    });
                    if records.len() > MAX_DATA_RECORDS + 1 {
                        return (
                            records,
                            Some(CsvParseFailure {
                                message: "record limit exceeded".into(),
                                line_start: record_start,
                                line_end: line,
                                fields: Vec::new(),
                                limit_exceeded: true,
                            }),
                        );
                    }
                    index += newline_len;
                    line += 1;
                    record_start = line;
                    record_has_input = false;
                }
                _ => {
                    field.push(ch);
                    state = State::Unquoted;
                    record_has_input = true;
                    index += 1;
                }
            },
            State::Unquoted => match ch {
                ',' => {
                    fields.push(std::mem::take(&mut field));
                    state = State::Start;
                    index += 1;
                }
                '\r' | '\n' => {
                    fields.push(std::mem::take(&mut field));
                    records.push(ParsedRecord {
                        fields: std::mem::take(&mut fields),
                        line_start: record_start,
                        line_end: line,
                        parse_issue: None,
                    });
                    if records.len() > MAX_DATA_RECORDS + 1 {
                        return (
                            records,
                            Some(CsvParseFailure {
                                message: "record limit exceeded".into(),
                                line_start: record_start,
                                line_end: line,
                                fields: Vec::new(),
                                limit_exceeded: true,
                            }),
                        );
                    }
                    index += newline_len;
                    line += 1;
                    record_start = line;
                    state = State::Start;
                    record_has_input = false;
                }
                '"' => {
                    return (
                        records,
                        Some(parse_failure(
                            "quote in an unquoted field",
                            record_start,
                            line,
                            fields,
                            field,
                        )),
                    )
                }
                _ => {
                    field.push(ch);
                    index += 1;
                }
            },
            State::Quoted => {
                if ch == '"' {
                    if chars.get(index + 1) == Some(&'"') {
                        field.push('"');
                        index += 2;
                    } else {
                        state = State::AfterQuote;
                        index += 1;
                    }
                } else {
                    field.push(ch);
                    if is_newline {
                        if ch == '\r' && newline_len == 2 {
                            field.push('\n');
                        }
                        index += newline_len;
                        line += 1;
                    } else {
                        index += 1;
                    }
                }
            }
            State::AfterQuote => match ch {
                ',' => {
                    fields.push(std::mem::take(&mut field));
                    state = State::Start;
                    index += 1;
                }
                '\r' | '\n' => {
                    fields.push(std::mem::take(&mut field));
                    records.push(ParsedRecord {
                        fields: std::mem::take(&mut fields),
                        line_start: record_start,
                        line_end: line,
                        parse_issue: None,
                    });
                    if records.len() > MAX_DATA_RECORDS + 1 {
                        return (
                            records,
                            Some(CsvParseFailure {
                                message: "record limit exceeded".into(),
                                line_start: record_start,
                                line_end: line,
                                fields: Vec::new(),
                                limit_exceeded: true,
                            }),
                        );
                    }
                    index += newline_len;
                    line += 1;
                    record_start = line;
                    state = State::Start;
                    record_has_input = false;
                }
                _ => {
                    return (
                        records,
                        Some(parse_failure(
                            "expected a comma or record terminator after a closing quote",
                            record_start,
                            line,
                            fields,
                            field,
                        )),
                    )
                }
            },
        }
    }

    if state == State::Quoted {
        return (
            records,
            Some(parse_failure(
                "unterminated quoted field",
                record_start,
                line,
                fields,
                field,
            )),
        );
    }
    if record_has_input
        || !fields.is_empty()
        || !field.is_empty()
        || state == State::AfterQuote
        || state == State::Unquoted
    {
        fields.push(field);
        records.push(ParsedRecord {
            fields,
            line_start: record_start,
            line_end: line,
            parse_issue: None,
        });
        if records.len() > MAX_DATA_RECORDS + 1 {
            return (
                records,
                Some(CsvParseFailure {
                    message: "record limit exceeded".into(),
                    line_start: record_start,
                    line_end: line,
                    fields: Vec::new(),
                    limit_exceeded: true,
                }),
            );
        }
    }
    (records, None)
}

fn is_empty_record(values: &[String]) -> bool {
    values.iter().all(|value| value.trim().is_empty())
}

fn header_errors(columns: &[String]) -> Vec<CsvIssue> {
    let expected: HashSet<&str> = EXPECTED_COLUMNS.iter().copied().collect();
    let mut counts = HashMap::<&str, usize>::with_capacity(columns.len());
    let mut duplicates = Vec::new();
    let mut missing = Vec::new();
    let mut unexpected = Vec::new();
    for column in columns {
        let count = counts.entry(column.as_str()).or_default();
        *count += 1;
        if *count == 2 {
            duplicates.push(column.clone());
        }
        if !expected.contains(column.as_str()) {
            unexpected.push(column.clone());
        }
    }
    for expected_column in EXPECTED_COLUMNS {
        if !counts.contains_key(expected_column) {
            missing.push(expected_column.to_string());
        }
    }
    duplicates.sort();
    let mut errors = Vec::new();
    if !duplicates.is_empty() {
        let mut issue = CsvIssue::code("duplicate_headers");
        issue.columns = Some(duplicates);
        errors.push(issue);
    }
    if !missing.is_empty() {
        let mut issue = CsvIssue::code("missing_headers");
        issue.columns = Some(missing);
        errors.push(issue);
    }
    if !unexpected.is_empty() {
        let mut issue = CsvIssue::code("unexpected_headers");
        issue.columns = Some(unexpected);
        errors.push(issue);
    }
    let has_missing_or_unexpected = errors
        .iter()
        .any(|issue| issue.code == "missing_headers" || issue.code == "unexpected_headers");
    if !has_missing_or_unexpected
        && columns.iter().map(String::as_str).collect::<Vec<_>>() != EXPECTED_COLUMNS
    {
        errors.push(CsvIssue::code("header_order_mismatch"));
    }
    errors
}

fn mapped_fields(columns: &[String], values: &[String]) -> MappedFields {
    let mut mapped = MappedFields::default();
    for (index, column) in columns.iter().enumerate() {
        let Some(value) = values.get(index).cloned() else {
            continue;
        };
        let target = match column.as_str() {
            "Track name" => &mut mapped.track_name,
            "Artist name" => &mut mapped.artist_name,
            "Album" => &mut mapped.album,
            "Playlist name" => &mut mapped.playlist_name,
            "Type" => &mut mapped.source_type,
            "ISRC" => &mut mapped.isrc,
            "Spotify - id" => &mut mapped.spotify_id,
            _ => continue,
        };
        if target.is_none() {
            *target = Some(value);
        }
    }
    mapped
}

fn duplicate_reports(rows: &mut [SpotifyCsvRow]) -> Vec<DuplicateReport> {
    let mut exact = BTreeMap::<Vec<String>, Vec<usize>>::new();
    let mut by_id = HashMap::<String, Vec<usize>>::new();
    for (index, row) in rows.iter().enumerate() {
        exact
            .entry(row.source_values.clone())
            .or_default()
            .push(index);
        let id = row.mapped.spotify_id.as_deref().unwrap_or("");
        if !id.trim().is_empty() {
            by_id.entry(id.to_string()).or_default().push(index);
        }
    }
    let mut reports = Vec::new();
    let mut add = |category: &str, indexes: &[usize], differing_fields: Option<Vec<String>>| {
        let mut source_records: Vec<_> = indexes
            .iter()
            .map(|index| rows[*index].source_record)
            .collect();
        source_records.sort_unstable();
        reports.push(DuplicateReport {
            report_id: String::new(),
            category: category.to_string(),
            source_records,
            differing_fields,
        });
    };
    for indexes in exact.values() {
        if indexes.len() > 1 {
            add("exact_row_duplicate", indexes, None);
        }
    }
    for indexes in by_id.values() {
        let fields = ["track_name", "artist_name", "album", "source_type"];
        let mut differing = Vec::new();
        for field in fields {
            let distinct: HashSet<Option<&str>> = indexes
                .iter()
                .map(|index| match field {
                    "track_name" => rows[*index].mapped.track_name.as_deref(),
                    "artist_name" => rows[*index].mapped.artist_name.as_deref(),
                    "album" => rows[*index].mapped.album.as_deref(),
                    _ => rows[*index].mapped.source_type.as_deref(),
                })
                .collect();
            if distinct.len() > 1 {
                differing.push(field.to_string());
            }
        }
        if indexes.len() > 1 {
            add("repeated_opaque_id", indexes, None);
            if !differing.is_empty() {
                add("opaque_id_metadata_conflict", indexes, Some(differing));
            }
        }
    }
    reports.sort_by(|a, b| {
        a.source_records
            .cmp(&b.source_records)
            .then(a.category.cmp(&b.category))
    });
    let mut by_record = HashMap::<usize, Vec<String>>::new();
    for (index, report) in reports.iter_mut().enumerate() {
        report.report_id = format!("duplicate-{}", index + 1);
        for source_record in &report.source_records {
            by_record
                .entry(*source_record)
                .or_default()
                .push(report.report_id.clone());
        }
    }
    for row in rows {
        row.duplicate_reports = by_record.remove(&row.source_record).unwrap_or_default();
    }
    reports
}

pub(crate) fn parse_spotify_csv(content: &[u8]) -> SpotifyCsvPreview {
    if content.len() > MAX_FILE_BYTES {
        return file_too_large(content.len());
    }
    let content = content.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(content);
    let input = match std::str::from_utf8(content) {
        Ok(input) => input,
        Err(error) => {
            let mut issue = CsvIssue::code("invalid_encoding");
            issue.message = Some(format!("Invalid UTF-8 at byte {}", error.valid_up_to()));
            return SpotifyCsvPreview {
                file_errors: vec![issue],
                ..SpotifyCsvPreview::default()
            };
        }
    };
    let (mut records, parse_failure) = parse_records(input);
    if parse_failure
        .as_ref()
        .is_some_and(|failure| failure.limit_exceeded)
    {
        let mut issue = CsvIssue::code("row_limit_exceeded");
        issue.max_records = Some(MAX_DATA_RECORDS);
        issue.observed_records = Some(MAX_DATA_RECORDS + 1);
        return SpotifyCsvPreview {
            complete: false,
            file_errors: vec![issue],
            ..SpotifyCsvPreview::default()
        };
    }
    if let Some(failure) = parse_failure.as_ref() {
        if !records.is_empty() {
            records.push(ParsedRecord {
                fields: failure.fields.clone(),
                line_start: failure.line_start,
                line_end: failure.line_end,
                parse_issue: Some(CsvIssue::parse_error(
                    failure.message.clone(),
                    failure.line_start,
                    failure.line_end,
                )),
            });
        }
    }
    if records.is_empty() && parse_failure.is_none() {
        return SpotifyCsvPreview {
            complete: true,
            file_errors: vec![CsvIssue::code("missing_header")],
            ..SpotifyCsvPreview::default()
        };
    }
    let mut records = records.into_iter();
    let header = records.next();
    let Some(header) = header else {
        let failure = parse_failure.expect("no header only occurs for a parser error");
        return SpotifyCsvPreview {
            file_errors: vec![CsvIssue::parse_error(
                failure.message,
                failure.line_start,
                failure.line_end,
            )],
            ..SpotifyCsvPreview::default()
        };
    };
    let columns = header.fields;
    let mut file_errors = header_errors(&columns);
    let mut rows = Vec::new();
    let mut ignored = 0;
    let mut nonempty_records = 0;
    let mut record_ordinal = 0;
    let complete = parse_failure.is_none();
    let mut fatal = parse_failure;
    let mut empty_spans: Vec<EmptyRecordSpan> = Vec::new();

    for record in records {
        record_ordinal += 1;
        if is_empty_record(&record.fields) {
            ignored += 1;
            if let Some(previous) = empty_spans.last_mut().filter(|span| {
                span.source_record_end + 1 == record_ordinal
                    && span.physical_line_end + 1 == record.line_start
            }) {
                previous.source_record_end = record_ordinal;
                previous.physical_line_end = record.line_end;
            } else {
                empty_spans.push(EmptyRecordSpan {
                    source_record_start: record_ordinal,
                    source_record_end: record_ordinal,
                    physical_line_start: record.line_start,
                    physical_line_end: record.line_end,
                });
            }
            continue;
        }
        nonempty_records += 1;
        if nonempty_records > MAX_DATA_RECORDS {
            let mut issue = CsvIssue::code("row_limit_exceeded");
            issue.max_records = Some(MAX_DATA_RECORDS);
            issue.observed_records = Some(nonempty_records);
            return SpotifyCsvPreview {
                complete: false,
                columns,
                summary: PreviewSummary {
                    source_records: record_ordinal,
                    preview_rows: 0,
                    ignored_empty_records: ignored,
                    ignored_empty_spans: empty_spans,
                    rows_with_errors: 0,
                    rows_with_warnings: 0,
                },
                file_errors: {
                    file_errors.push(issue);
                    file_errors
                },
                rows: vec![],
                duplicate_reports: vec![],
            };
        }
        let mapped = mapped_fields(&columns, &record.fields);
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        if let Some(issue) = record.parse_issue {
            errors.push(issue);
        }
        if record.fields.len() != EXPECTED_COLUMNS.len() {
            let mut issue = CsvIssue::code("field_count_mismatch");
            issue.expected = Some(EXPECTED_COLUMNS.len());
            issue.actual = Some(record.fields.len());
            errors.push(issue);
        }
        if mapped
            .source_type
            .as_deref()
            .is_none_or(|kind| kind.trim().is_empty())
        {
            errors.push(CsvIssue::code("missing_type"));
        }
        for (key, value) in [
            ("track_name", &mapped.track_name),
            ("artist_name", &mapped.artist_name),
            ("album", &mapped.album),
        ] {
            if value.as_deref().is_none_or(|text| text.trim().is_empty()) {
                let mut issue = CsvIssue::code("blank_descriptive_field");
                issue.field = Some(key.to_string());
                warnings.push(issue);
            }
        }
        rows.push(SpotifyCsvRow {
            source_record: record_ordinal,
            physical_line_start: record.line_start,
            physical_line_end: record.line_end,
            source_values: record.fields,
            mapped,
            errors,
            warnings,
            duplicate_reports: vec![],
        });
    }
    if let Some(failure) = fatal.take() {
        file_errors.push(CsvIssue::parse_error(
            failure.message,
            failure.line_start,
            failure.line_end,
        ));
    }
    let duplicate_reports = duplicate_reports(&mut rows);
    SpotifyCsvPreview {
        complete,
        columns,
        summary: PreviewSummary {
            source_records: record_ordinal,
            preview_rows: rows.len(),
            ignored_empty_records: ignored,
            ignored_empty_spans: empty_spans,
            rows_with_errors: rows.iter().filter(|row| !row.errors.is_empty()).count(),
            rows_with_warnings: rows.iter().filter(|row| !row.warnings.is_empty()).count(),
        },
        file_errors,
        rows,
        duplicate_reports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const HEADER: &str = "Track name,Artist name,Album,Playlist name,Type,ISRC,Spotify - id\r\n";
    fn row(
        name: &str,
        artist: &str,
        album: &str,
        playlist: &str,
        kind: &str,
        isrc: &str,
        id: &str,
    ) -> String {
        format!("{name},{artist},{album},{playlist},{kind},{isrc},{id}\r\n")
    }

    fn temp_preview_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "mypod-spotify-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn token_scoped_preview_cleans_its_staging_file_without_creating_a_catalog() {
        let root = temp_preview_root("staging");
        let staging = root.join(APP_FOLDER).join(STAGING_FOLDER);
        fs::create_dir_all(&staging).unwrap();
        let id = "0123456789abcdef0123456789abcdef";
        let path = staging.join(format!("spotify-{id}.csv.part"));
        fs::write(
            &path,
            format!(
                "{HEADER}{}",
                row("Synthetic", "Artist", "Album", "", "opaque", "", "id")
            ),
        )
        .unwrap();

        let result = preview_staged_file(&root, id).unwrap();
        assert!(result.complete);
        assert_eq!(
            result.rows[0].mapped.track_name.as_deref(),
            Some("Synthetic")
        );
        assert!(!path.exists());
        assert!(!root.join(APP_FOLDER).join("library.sqlite3").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn token_scoped_preview_rejects_symlinked_staging_files() {
        use std::os::unix::fs::symlink;

        let root = temp_preview_root("symlink");
        let staging = root.join(APP_FOLDER).join(STAGING_FOLDER);
        fs::create_dir_all(&staging).unwrap();
        let id = "fedcba9876543210fedcba9876543210";
        let path = staging.join(format!("spotify-{id}.csv.part"));
        let outside = root.join("outside.csv");
        fs::write(&outside, format!("{HEADER}")).unwrap();
        symlink(&outside, &path).unwrap();

        assert!(preview_staged_file(&root, id).is_err());
        assert!(outside.exists());
        assert!(!path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_quoted_commas_newlines_and_physical_spans() {
        let input =
            format!("{HEADER}\"Song, one\",\"Artist\r\nwith line\",Album,Mix,Playlist,,opaque\r\n");
        let result = parse_spotify_csv(input.as_bytes());
        assert!(result.complete);
        assert_eq!(
            result.rows[0].mapped.track_name.as_deref(),
            Some("Song, one")
        );
        assert_eq!(
            result.rows[0].mapped.artist_name.as_deref(),
            Some("Artist\r\nwith line")
        );
        assert_eq!(
            (
                result.rows[0].physical_line_start,
                result.rows[0].physical_line_end
            ),
            (2, 3)
        );
    }

    #[test]
    fn accepts_utf8_bom_and_rejects_invalid_utf8() {
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(
            format!("{HEADER}{}", row("A", "B", "C", "D", "Playlist", "", "")).as_bytes(),
        );
        assert!(parse_spotify_csv(&bom).complete);
        let result = parse_spotify_csv(b"\xff\xfe");
        assert!(!result.complete);
        assert_eq!(result.file_errors[0].code, "invalid_encoding");
    }

    #[test]
    fn malformed_quote_returns_partial_rows_and_incomplete_status() {
        let input = format!(
            "{HEADER}{}\"unfinished\r\n",
            row("Good", "A", "B", "Mix", "Playlist", "", "x")
        );
        let result = parse_spotify_csv(input.as_bytes());
        assert!(!result.complete);
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[1].source_record, 2);
        assert!(result.rows[1]
            .errors
            .iter()
            .any(|error| error.code == "csv_parse_error"));
        assert_eq!(result.file_errors.last().unwrap().code, "csv_parse_error");
        assert_eq!(
            result.file_errors.last().unwrap().physical_line_start,
            Some(3)
        );
    }

    #[test]
    fn validates_missing_duplicate_reordered_and_unexpected_headers() {
        let reordered = "Artist name,Track name,Album,Playlist name,Type,ISRC,Spotify - id\nA,T,B,M,Playlist,,x\n";
        assert!(parse_spotify_csv(reordered.as_bytes())
            .file_errors
            .iter()
            .any(|e| e.code == "header_order_mismatch"));
        let missing = "Track name,Artist name,Album,Playlist name,Type,ISRC\nT,A,B,M,Playlist,\n";
        assert!(parse_spotify_csv(missing.as_bytes())
            .file_errors
            .iter()
            .any(|e| e.code == "missing_headers"));
        let duplicate = "Track name,Artist name,Album,Playlist name,Type,ISRC,Track name\nT,A,B,M,Playlist,,x\n";
        let duplicate_result = parse_spotify_csv(duplicate.as_bytes());
        assert!(duplicate_result
            .file_errors
            .iter()
            .any(|e| e.code == "duplicate_headers"));
        assert!(duplicate_result
            .file_errors
            .iter()
            .any(|e| e.code == "missing_headers"));
        let unexpected = "Track name,Artist name,Album,Playlist name,Type,ISRC,Spotify - id,Extra\nT,A,B,M,Playlist,,x,y\n";
        let unexpected_result = parse_spotify_csv(unexpected.as_bytes());
        assert!(unexpected_result
            .file_errors
            .iter()
            .any(|e| e.code == "unexpected_headers"));
        assert!(!unexpected_result
            .file_errors
            .iter()
            .any(|e| e.code == "missing_headers"));
    }

    #[test]
    fn keeps_empty_record_count_spans_source_ordinals_and_opaque_values() {
        let input = format!(
            "{HEADER}\r\n{}",
            row(
                "Synthetic",
                "Artist",
                "Album",
                "Same label",
                "Unrecognized",
                "",
                "opaque-id"
            )
        );
        let result = parse_spotify_csv(input.as_bytes());
        assert_eq!(result.summary.ignored_empty_records, 1);
        assert_eq!(
            result.summary.ignored_empty_spans,
            vec![EmptyRecordSpan {
                source_record_start: 1,
                source_record_end: 1,
                physical_line_start: 2,
                physical_line_end: 2
            }]
        );
        assert_eq!(result.rows[0].source_record, 2);
        assert_eq!(
            result.rows[0].mapped.spotify_id.as_deref(),
            Some("opaque-id")
        );
        assert_eq!(
            result.rows[0].mapped.source_type.as_deref(),
            Some("Unrecognized")
        );
        assert!(!result.rows[0]
            .warnings
            .iter()
            .any(|warning| warning.code == "unsupported_type"));
    }

    #[test]
    fn reports_duplicate_categories_and_conflicts_without_merging_rows() {
        let a = row("One", "Artist", "Album", "Mix A", "Playlist", "", "same-id");
        let input = format!(
            "{HEADER}{a}{a}{}",
            row(
                "One",
                "Different artist",
                "Album",
                "Mix B",
                "Favorite",
                "",
                "same-id"
            )
        );
        let result = parse_spotify_csv(input.as_bytes());
        assert_eq!(result.rows.len(), 3);
        let categories: HashSet<_> = result
            .duplicate_reports
            .iter()
            .map(|report| report.category.as_str())
            .collect();
        assert!(categories.contains("exact_row_duplicate"));
        assert!(categories.contains("repeated_opaque_id"));
        assert!(!categories.contains("repeated_id_within_playlist"));
        assert!(!categories.contains("id_across_playlists"));
        assert!(categories.contains("opaque_id_metadata_conflict"));
    }

    #[test]
    fn opaque_ids_are_compared_as_exact_source_values_without_playlist_inference() {
        let input = format!(
            "{HEADER}{}{}",
            row(
                "A",
                "Artist",
                "Album",
                "Same label",
                "Type A",
                "",
                "opaque-id"
            ),
            row(
                "B",
                "Artist",
                "Album",
                "Same label",
                "Type B",
                "",
                " opaque-id"
            )
        );
        let result = parse_spotify_csv(input.as_bytes());
        assert!(result.duplicate_reports.is_empty());
        assert_eq!(result.rows[0].mapped.source_type.as_deref(), Some("Type A"));
        assert_eq!(result.rows[1].mapped.source_type.as_deref(), Some("Type B"));
    }

    #[test]
    fn accepts_exact_limits_and_rejects_one_over_without_truncation() {
        let prefix = format!("{HEADER}\"");
        let suffix = "\",A,B,M,Playlist,,x\r\n";
        let padding = MAX_FILE_BYTES - prefix.len() - suffix.len();
        let exact = format!("{prefix}{}{suffix}", "x".repeat(padding));
        assert_eq!(exact.len(), MAX_FILE_BYTES);
        assert!(parse_spotify_csv(exact.as_bytes()).complete);
        let over = format!("{exact} ");
        let too_large = parse_spotify_csv(over.as_bytes());
        assert!(!too_large.complete);
        assert_eq!(too_large.file_errors[0].code, "file_too_large");
        assert!(too_large.rows.is_empty());

        let records = row("T", "A", "B", "M", "Playlist", "", "x").repeat(MAX_DATA_RECORDS);
        let at_limit = parse_spotify_csv(format!("{HEADER}{records}").as_bytes());
        assert!(at_limit.complete);
        assert_eq!(at_limit.rows.len(), MAX_DATA_RECORDS);
        let over_limit = parse_spotify_csv(
            format!(
                "{HEADER}{records}{}",
                row("extra", "A", "B", "M", "Playlist", "", "y")
            )
            .as_bytes(),
        );
        assert!(!over_limit.complete);
        assert_eq!(
            over_limit.file_errors.last().unwrap().code,
            "row_limit_exceeded"
        );
        assert!(over_limit.rows.is_empty());
    }
}
