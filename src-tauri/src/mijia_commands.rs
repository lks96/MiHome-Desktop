use crate::{
    errors::ApiResult,
    services::mijia::{LoginProgress, MiCloud, MiProperty, MiSnapshot, QrLogin},
};
use tauri::Manager;

#[tauri::command]
pub async fn camera_remote_start(
    service: tauri::State<'_, MiCloud>,
    window: tauri::WebviewWindow,
    windows: tauri::State<'_, crate::camera_windows::CameraWindows>,
    did: String,
    quality: Option<u8>,
) -> Result<ApiResult<crate::services::camera_stream::StreamStatus>, ()> {
    if !windows.permits(window.label(), &did).await {
        return Ok(Err(crate::errors::AppError::invalid_input(
            "此窗口不能播放该设备",
        ))
        .into());
    }
    let result = service
        .camera_remote_start_owned(&did, quality.unwrap_or(2), window.label())
        .await;
    if let Ok(status) = &result {
        if window
            .app_handle()
            .get_webview_window(window.label())
            .is_none()
        {
            service
                .cameras
                .stop_owned(&status.session_id, window.label())
                .await;
            return Ok(Err(crate::errors::AppError::new(
                "CAMERA_WINDOW_GONE",
                "观看窗口已关闭",
                false,
            ))
            .into());
        }
    }
    Ok(result.into())
}
#[tauri::command]
pub async fn camera_remote_status(
    service: tauri::State<'_, MiCloud>,
    window: tauri::WebviewWindow,
    session_id: String,
) -> Result<ApiResult<crate::services::camera_stream::StreamStatus>, ()> {
    Ok(service
        .cameras
        .status_owned(&session_id, window.label())
        .await
        .into())
}
#[tauri::command]
pub async fn camera_remote_stop(
    service: tauri::State<'_, MiCloud>,
    window: tauri::WebviewWindow,
    session_id: String,
) -> Result<ApiResult<bool>, ()> {
    service
        .cameras
        .stop_owned(&session_id, window.label())
        .await;
    Ok(Ok(true).into())
}

#[tauri::command]
pub async fn mijia_camera_probe(
    service: tauri::State<'_, MiCloud>,
    did: String,
) -> Result<ApiResult<crate::services::mijia_camera::CameraProbeReport>, ()> {
    Ok(service.camera_probe(&did).await.into())
}

#[tauri::command]
pub async fn mijia_restore(
    service: tauri::State<'_, MiCloud>,
) -> Result<ApiResult<MiSnapshot>, ()> {
    Ok(service.restore().await.into())
}
#[tauri::command]
pub async fn mijia_start_login(
    service: tauri::State<'_, MiCloud>,
    region: String,
    remember: bool,
) -> Result<ApiResult<QrLogin>, ()> {
    Ok(service.start_login(region, remember).await.into())
}
#[tauri::command]
pub async fn mijia_poll_login(
    service: tauri::State<'_, MiCloud>,
    id: String,
) -> Result<ApiResult<LoginProgress>, ()> {
    Ok(service.poll_login(&id).await.into())
}
#[tauri::command]
pub async fn mijia_cancel_login(
    service: tauri::State<'_, MiCloud>,
    id: String,
) -> Result<ApiResult<bool>, ()> {
    Ok(Ok(service.cancel_login(&id).await).into())
}
#[tauri::command]
pub async fn mijia_logout(
    service: tauri::State<'_, MiCloud>,
    app: tauri::AppHandle,
    windows: tauri::State<'_, crate::camera_windows::CameraWindows>,
) -> Result<ApiResult<bool>, ()> {
    let result = service.logout().await;
    windows.close_all(&app).await;
    Ok(result.into())
}
#[tauri::command]
pub async fn mijia_sync(service: tauri::State<'_, MiCloud>) -> Result<ApiResult<MiSnapshot>, ()> {
    Ok(service.sync().await.into())
}
#[tauri::command]
pub async fn mijia_get_properties(
    service: tauri::State<'_, MiCloud>,
    did: String,
) -> Result<ApiResult<Vec<MiProperty>>, ()> {
    Ok(service.properties(&did).await.into())
}
#[tauri::command]
pub async fn mijia_set_property(
    service: tauri::State<'_, MiCloud>,
    did: String,
    siid: u32,
    piid: u32,
    value: serde_json::Value,
) -> Result<ApiResult<Vec<MiProperty>>, ()> {
    Ok(service.set_property(&did, siid, piid, value).await.into())
}
