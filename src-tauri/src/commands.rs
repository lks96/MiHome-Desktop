// Tauri async State commands require native Result; successful transport carries ApiResult.
use crate::{database::Database, errors::ApiResult, models::AppSettings};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
    platform: &'static str,
}
#[tauri::command]
pub fn settings_get_app_info() -> ApiResult<AppInfo> {
    Ok(AppInfo {
        name: "Mi Home Desktop",
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
    })
    .into()
}
#[tauri::command]
pub async fn settings_get(
    database: tauri::State<'_, Database>,
) -> Result<ApiResult<AppSettings>, ()> {
    Ok(database.settings().await.into())
}
#[tauri::command]
pub async fn settings_update(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, crate::desktop::DesktopState>,
    database: tauri::State<'_, Database>,
    settings: AppSettings,
) -> Result<ApiResult<AppSettings>, ()> {
    Ok(
        crate::desktop::save_settings(&app, &runtime, &database, settings)
            .await
            .into(),
    )
}

use crate::services::{
    ha::{self, ConnectionSummary},
    session::{ConnectionStatus, EntitySummary, HaService},
};
use zeroize::Zeroizing;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    message: &'static str,
}

#[tauri::command]
pub async fn ha_test_connection(
    database: tauri::State<'_, Database>,
    base_url: String,
    token: String,
) -> Result<ApiResult<ConnectionTestResult>, ()> {
    Ok({
        let token = Zeroizing::new(token);
        let result = async {
            let settings = database.settings().await?;
            ha::test_connection(&base_url, &token, settings.request_timeout).await?;
            Ok(ConnectionTestResult {
                message: "Home Assistant API 认证成功",
            })
        }
        .await;
        result.into()
    })
}
#[tauri::command]
pub async fn ha_save_connection(
    database: tauri::State<'_, Database>,
    service: tauri::State<'_, HaService>,
    name: String,
    base_url: String,
    token: String,
) -> Result<ApiResult<ConnectionSummary>, ()> {
    Ok({
        let token = Zeroizing::new(token);
        let _operation = service.operation.lock().await;
        database
            .save_connection(service.credentials.as_ref(), &name, &base_url, token)
            .await
            .into()
    })
}
#[tauri::command]
pub async fn ha_get_saved_connection(
    database: tauri::State<'_, Database>,
) -> Result<ApiResult<Option<ConnectionSummary>>, ()> {
    Ok(database
        .connection()
        .await
        .map(|value| value.map(|connection| connection.summary()))
        .into())
}
#[tauri::command]
pub async fn ha_get_connection_status(
    service: tauri::State<'_, HaService>,
) -> Result<ApiResult<ConnectionStatus>, ()> {
    Ok(Ok(service.status().await).into())
}
#[tauri::command]
pub async fn ha_connect(
    database: tauri::State<'_, Database>,
    service: tauri::State<'_, HaService>,
    connection_id: String,
) -> Result<ApiResult<ConnectionStatus>, ()> {
    Ok({
        if connection_id != "primary" {
            return Ok(Err(crate::errors::AppError::invalid_input("未知连接")).into());
        }
        service.connect(&database).await.into()
    })
}
#[tauri::command]
pub async fn ha_disconnect(service: tauri::State<'_, HaService>) -> Result<ApiResult<bool>, ()> {
    Ok({
        service.disconnect().await;
        Ok(true).into()
    })
}
#[tauri::command]
pub async fn ha_get_entities(
    service: tauri::State<'_, HaService>,
) -> Result<ApiResult<Vec<EntitySummary>>, ()> {
    Ok(Ok(service.entities().await).into())
}

use crate::services::{
    catalog::DeviceCatalog,
    devices::{EntityPreference, ServiceRequest, ServiceResult},
};
#[tauri::command]
pub async fn ha_get_catalog(
    service: tauri::State<'_, HaService>,
) -> Result<ApiResult<DeviceCatalog>, ()> {
    Ok(Ok(service.catalog().await).into())
}
#[tauri::command]
pub async fn device_get_preferences(
    database: tauri::State<'_, Database>,
) -> Result<ApiResult<Vec<EntityPreference>>, ()> {
    Ok(database.entity_preferences().await.into())
}
#[tauri::command]
pub async fn device_update_preferences(
    database: tauri::State<'_, Database>,
    preference: EntityPreference,
) -> Result<ApiResult<EntityPreference>, ()> {
    Ok(database.update_entity_preference(preference).await.into())
}
#[tauri::command]
pub async fn device_call_service(
    database: tauri::State<'_, Database>,
    service: tauri::State<'_, HaService>,
    request: ServiceRequest,
) -> Result<ApiResult<ServiceResult>, ()> {
    Ok(service.call_service(&database, request).await.into())
}

use crate::services::cameras::{
    CameraProfile, CameraSession, CameraSessions, Gateway, Probe, Snapshot,
};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraConfiguration {
    gateway: Gateway,
    profiles: Vec<CameraProfile>,
}
#[tauri::command]
pub async fn camera_get_configuration(
    database: tauri::State<'_, Database>,
) -> Result<ApiResult<CameraConfiguration>, ()> {
    Ok(async {
        Ok(CameraConfiguration {
            gateway: database.camera_gateway().await?,
            profiles: database.camera_profiles().await?,
        })
    }
    .await
    .into())
}
#[tauri::command]
pub async fn camera_save_gateway(
    database: tauri::State<'_, Database>,
    sessions: tauri::State<'_, CameraSessions>,
    gateway: Gateway,
) -> Result<ApiResult<Gateway>, ()> {
    Ok({
        let result = database.save_camera_gateway(gateway).await;
        if result.is_ok() {
            sessions.clear().await;
        }
        result.into()
    })
}
#[tauri::command]
pub async fn camera_probe_gateway(gateway: Gateway) -> Result<ApiResult<Probe>, ()> {
    Ok(crate::services::cameras::probe_gateway(&gateway, 10)
        .await
        .into())
}
#[tauri::command]
pub async fn camera_save_profile(
    database: tauri::State<'_, Database>,
    sessions: tauri::State<'_, CameraSessions>,
    profile: CameraProfile,
) -> Result<ApiResult<CameraProfile>, ()> {
    Ok({
        let result = database.save_camera_profile(profile).await;
        if result.is_ok() {
            sessions.clear().await;
        }
        result.into()
    })
}
#[tauri::command]
pub async fn camera_get_snapshot(
    database: tauri::State<'_, Database>,
    service: tauri::State<'_, HaService>,
    entity_id: String,
) -> Result<ApiResult<Snapshot>, ()> {
    Ok(service.camera_snapshot(&database, &entity_id).await.into())
}
#[tauri::command]
pub async fn camera_start_session(
    database: tauri::State<'_, Database>,
    sessions: tauri::State<'_, CameraSessions>,
    entity_id: String,
) -> Result<ApiResult<CameraSession>, ()> {
    Ok(sessions.start(&database, &entity_id).await.into())
}
#[tauri::command]
pub async fn camera_exchange_sdp(
    sessions: tauri::State<'_, CameraSessions>,
    session_id: String,
    offer: String,
) -> Result<ApiResult<String>, ()> {
    Ok(sessions.exchange(&session_id, &offer).await.into())
}
#[tauri::command]
pub async fn camera_keep_session(
    sessions: tauri::State<'_, CameraSessions>,
    session_id: String,
) -> Result<ApiResult<bool>, ()> {
    Ok(sessions.keep(&session_id).await.into())
}
#[tauri::command]
pub async fn camera_stop_session(
    sessions: tauri::State<'_, CameraSessions>,
    session_id: String,
) -> Result<ApiResult<bool>, ()> {
    Ok(Ok(sessions.stop(&session_id).await).into())
}

use crate::services::scenes::{SceneFavorite, SceneSummary};
#[tauri::command]
pub async fn scene_get_list(
    database: tauri::State<'_, Database>,
    service: tauri::State<'_, HaService>,
) -> Result<ApiResult<Vec<SceneSummary>>, ()> {
    Ok(service.scenes(&database).await.into())
}
#[tauri::command]
pub async fn scene_get_favorites(
    database: tauri::State<'_, Database>,
) -> Result<ApiResult<Vec<SceneFavorite>>, ()> {
    Ok(database.scene_favorites().await.into())
}
#[tauri::command]
pub async fn scene_set_favorite(
    database: tauri::State<'_, Database>,
    scene_entity_id: String,
    favorite: bool,
) -> Result<ApiResult<SceneFavorite>, ()> {
    Ok(database
        .set_scene_favorite(&scene_entity_id, favorite)
        .await
        .into())
}
#[tauri::command]
pub async fn scene_execute(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    service: tauri::State<'_, HaService>,
    scene_entity_id: String,
) -> Result<ApiResult<ServiceResult>, ()> {
    Ok({
        use tauri::Emitter;
        let result = service.execute_scene(&database, &scene_entity_id).await;
        if result.is_ok() {
            let _ = app.emit(
                "scene:executed",
                serde_json::json!({"entityId":scene_entity_id,"accepted":true}),
            );
        }
        result.into()
    })
}
#[tauri::command]
pub fn desktop_get_status(app: tauri::AppHandle) -> ApiResult<crate::desktop::DesktopStatus> {
    Ok(crate::desktop::status(&app)).into()
}
#[tauri::command]
pub async fn settings_get_diagnostics(
    service: tauri::State<'_, HaService>,
) -> Result<ApiResult<crate::services::diagnostics::DiagnosticInfo>, ()> {
    Ok(Ok(crate::services::diagnostics::info(&service).await).into())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    cancelled: bool,
    file_name: Option<String>,
}
#[tauri::command]
pub async fn settings_export_logs(
    app: tauri::AppHandle,
    service: tauri::State<'_, HaService>,
    acknowledged: bool,
) -> Result<ApiResult<ExportResult>, ()> {
    Ok({
        use crate::services::diagnostics::{self, DiagnosticReport};
        use tauri::Manager;
        use tauri_plugin_dialog::DialogExt;
        let result = async {
            if !acknowledged {
                return Err(crate::errors::AppError::invalid_input(
                    "请先确认诊断包隐私提示",
                ));
            }
            let summary = diagnostics::info(&service).await;
            let directory = app
                .path()
                .app_log_dir()
                .map_err(|_| diagnostics::export_error())?;
            let chooser = app.clone();
            let file = tauri::async_runtime::spawn_blocking(move || {
                chooser
                    .dialog()
                    .file()
                    .add_filter("JSON 诊断包", &["json"])
                    .set_file_name("MiHome-diagnostics.json")
                    .blocking_save_file()
            })
            .await
            .map_err(|_| diagnostics::export_error())?;
            let Some(file) = file else {
                return Ok(ExportResult {
                    cancelled: true,
                    file_name: None,
                });
            };
            let path = file.into_path().map_err(|_| diagnostics::export_error())?;
            tauri::async_runtime::spawn_blocking(move || {
                use std::io::Write;
                let report = DiagnosticReport {
                    schema_version: 1,
                    summary,
                    logs: diagnostics::collect_logs(&directory)?,
                };
                let parent = path.parent().ok_or_else(diagnostics::export_error)?;
                let mut temporary = tempfile::NamedTempFile::new_in(parent)
                    .map_err(|_| diagnostics::export_error())?;
                temporary
                    .write_all(
                        &serde_json::to_vec_pretty(&report)
                            .map_err(|_| diagnostics::export_error())?,
                    )
                    .map_err(|_| diagnostics::export_error())?;
                temporary
                    .as_file()
                    .sync_all()
                    .map_err(|_| diagnostics::export_error())?;
                temporary
                    .persist(&path)
                    .map_err(|_| diagnostics::export_error())?;
                Ok(ExportResult {
                    cancelled: false,
                    file_name: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned()),
                })
            })
            .await
            .map_err(|_| diagnostics::export_error())?
        }
        .await;
        result.into()
    })
}
