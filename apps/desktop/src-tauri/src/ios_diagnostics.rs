#[cfg(target_os = "ios")]
use tauri::Manager;

use serde::Serialize;
use std::fs;
use std::path::Path;

const NATIVE_BUILD_ID: &str = "ios-diagnostics-2026-10-19-r1";

#[derive(Debug, Clone, Serialize)]
pub struct IosRuntimeDiagnostics {
    pub bridge_works: bool,
    pub native_build_id: &'static str,
    pub database_schema_version: Option<i64>,
    pub migration_status: String,
    pub csv_selection_returned_path: bool,
    pub selected_file_exists: Option<bool>,
    pub staging_succeeded: bool,
    pub preview_succeeded: bool,
    pub failure_stage: String,
    pub sanitized_error: Option<String>,
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub fn ios_runtime_diagnostics(
    app: tauri::AppHandle,
    selected_path: Option<String>,
    staging_succeeded: Option<bool>,
    preview_succeeded: Option<bool>,
) -> IosRuntimeDiagnostics {
    let mut result = IosRuntimeDiagnostics {
        bridge_works: true,
        native_build_id: NATIVE_BUILD_ID,
        database_schema_version: None,
        migration_status: "not_checked".into(),
        csv_selection_returned_path: selected_path.is_some(),
        selected_file_exists: None,
        staging_succeeded: staging_succeeded.unwrap_or(false),
        preview_succeeded: preview_succeeded.unwrap_or(false),
        failure_stage: "bridge".into(),
        sanitized_error: None,
    };
    let db_path = match app.path().app_local_data_dir() {
        Ok(path) => path.join("MyPod").join("library.sqlite3"),
        Err(_) => {
            result.failure_stage = "database_path".into();
            result.migration_status = "unable_to_resolve_app_data".into();
            return result;
        }
    };
    match rusqlite::Connection::open(db_path) {
        Ok(db) => match db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0)) {
            Ok(version) => {
                result.database_schema_version = Some(version);
                result.migration_status = if version == 4 {
                    "current"
                } else {
                    "requires_migration"
                }
                .into();
                result.failure_stage = if version == 4 {
                    "database"
                } else {
                    "migration"
                }
                .into();
            }
            Err(_) => {
                result.failure_stage = "database_schema".into();
                result.migration_status = "unable_to_read_user_version".into();
            }
        },
        Err(_) => {
            result.failure_stage = "database_open".into();
            result.migration_status = "database_unavailable".into();
        }
    }
    if let Some(path) = selected_path {
        let is_file = fs::metadata(Path::new(&path))
            .map(|m| m.is_file())
            .unwrap_or(false);
        result.selected_file_exists = Some(is_file);
        result.failure_stage = if result.staging_succeeded && result.preview_succeeded {
            "complete"
        } else {
            "file_access"
        }
        .into();
        if !is_file && !result.staging_succeeded {
            result.sanitized_error =
                Some("Selected file was not readable by the native process".into());
        }
    } else if result.failure_stage == "database" {
        result.failure_stage = if result.staging_succeeded && result.preview_succeeded {
            "complete"
        } else {
            "selection"
        }
        .into();
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn diagnostic_build_identifier_is_stable_for_runtime_checks() {
        assert_eq!(super::NATIVE_BUILD_ID, "ios-diagnostics-2026-10-19-r1");
    }
}
