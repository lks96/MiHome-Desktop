use crate::{
    errors::ApiResult,
    services::mijia::{LoginProgress, MiCloud, MiProperty, MiSnapshot, QrLogin},
};

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
pub async fn mijia_logout(service: tauri::State<'_, MiCloud>) -> Result<ApiResult<bool>, ()> {
    Ok(service.logout().await.into())
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
