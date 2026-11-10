use crate::errors::AppError;
use serde::Serialize;
use serde_json::Value;
use std::{io::Read, path::Path};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeLog {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticLogs {
    pub entries: Vec<SafeLog>,
    pub omitted_lines: usize,
}

fn safe_line(line: &str) -> Option<SafeLog> {
    let value: Value = serde_json::from_str(line).ok()?;
    if !value["target"].as_str()?.starts_with("mi_home_desktop_lib") {
        return None;
    }
    let message = value["fields"]["message"].as_str()?;
    // Reviewed static messages only; never export arbitrary text, SQL args,
    // paths, URLs, entity names, auth headers, SDP, or credential fields.
    if ![
        "Application initialized",
        "Local settings database operation failed",
        "HA service call accepted",
        "Unable to remove superseded credential",
        "Unable to deliver Home Assistant UI event",
        "System tray unavailable",
        "Unable to restore startup preference",
        "Unable to hide main window",
    ]
    .contains(&message)
    {
        return None;
    }
    let level = value["level"].as_str()?;
    if !["TRACE", "DEBUG", "INFO", "WARN", "ERROR"].contains(&level) {
        return None;
    }
    let timestamp = chrono::DateTime::parse_from_rfc3339(value["timestamp"].as_str()?)
        .ok()?
        .to_rfc3339();
    Some(SafeLog {
        timestamp,
        level: level.into(),
        message: message.into(),
    })
}
pub fn collect_logs(directory: &Path) -> Result<DiagnosticLogs, AppError> {
    if !directory.exists() {
        return Ok(DiagnosticLogs::default());
    }
    let mut files = std::fs::read_dir(directory)
        .map_err(|_| export_error())?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("mihome") && name.ends_with(".log")
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|entry| std::cmp::Reverse(entry.file_name()));
    let mut logs = DiagnosticLogs::default();
    let mut total = 0;
    for entry in files.into_iter().take(7) {
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(|_| export_error())?;
        if !metadata.is_file()
            || metadata.len() > 1024 * 1024
            || total + metadata.len() > 4 * 1024 * 1024
        {
            continue;
        }
        let mut body = Vec::new();
        std::fs::File::open(entry.path())
            .map_err(|_| export_error())?
            .take(1024 * 1024)
            .read_to_end(&mut body)
            .map_err(|_| export_error())?;
        total += body.len() as u64;
        for line in String::from_utf8_lossy(&body).lines() {
            if line.len() > 16384 {
                logs.omitted_lines += 1;
                continue;
            }
            match safe_line(line) {
                Some(entry) => logs.entries.push(entry),
                None => logs.omitted_lines += 1,
            }
        }
    }
    Ok(logs)
}
pub fn export_error() -> AppError {
    AppError::new(
        "LOG_EXPORT_FAILED",
        "无法读取或保存诊断包，请检查文件权限与可用空间",
        true,
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticInfo {
    pub version: &'static str,
    pub platform: &'static str,
    pub connection_state: super::session::ConnectionState,
    pub retry_attempt: u32,
    pub error_code: Option<&'static str>,
    pub entity_count: usize,
    pub camera_count: usize,
    pub scene_count: usize,
}
pub async fn info(service: &super::session::HaService) -> DiagnosticInfo {
    let status = service.status().await;
    let entities = service.entities().await;
    DiagnosticInfo {
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        connection_state: status.state,
        retry_attempt: status.attempt,
        error_code: status.error.map(|error| error.code),
        entity_count: entities.len(),
        camera_count: entities
            .iter()
            .filter(|entity| entity.domain == "camera")
            .count(),
        scene_count: entities
            .iter()
            .filter(|entity| entity.domain == "scene")
            .count(),
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub schema_version: u32,
    pub summary: DiagnosticInfo,
    pub logs: DiagnosticLogs,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exports_only_reviewed_fields_and_omits_legacy_or_unknown_messages() {
        let dir = tempfile::tempdir().unwrap();
        let safe = serde_json::json!({"timestamp":"2026-11-10T01:00:00Z","level":"INFO","target":"mi_home_desktop_lib::commands","fields":{"message":"HA service call accepted","token":"fake-secret-token","Authorization":"Bearer fake-secret-token","url":"http://user:password@private/","entity":"private entity"}});
        let unsafe_line = serde_json::json!({"timestamp":"2026-11-10T01:00:00Z","level":"INFO","target":"mi_home_desktop_lib","fields":{"message":"Bearer fake-secret-token"}});
        std::fs::write(
            dir.path().join("mihome.2026-11-10.log"),
            format!("{safe}\n{unsafe_line}\nlegacy token=fake-secret-token\n"),
        )
        .unwrap();
        let result = collect_logs(dir.path()).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.omitted_lines, 2);
        let exported = serde_json::to_string(&result).unwrap();
        for secret in [
            "fake-secret-token",
            "password",
            "private entity",
            "Authorization",
        ] {
            assert!(!exported.contains(secret));
        }
    }
    #[cfg(unix)]
    #[test]
    fn log_export_does_not_follow_symlinked_files() {
        let dir = tempfile::tempdir().unwrap();
        let external = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(external.path(), dir.path().join("mihome.link.log")).unwrap();
        assert!(collect_logs(dir.path()).unwrap().entries.is_empty());
    }
    #[test]
    fn old_settings_deserialize_with_safe_desktop_defaults() {
        let settings: crate::models::AppSettings = serde_json::from_str(
            r#"{"theme":"light","requestTimeout":10,"autoConnect":true,"autoReconnect":true}"#,
        )
        .unwrap();
        assert!(!settings.close_to_tray && !settings.auto_start);
    }
}
