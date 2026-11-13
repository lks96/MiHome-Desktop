//! Dedicated camera viewer windows. Device/session ownership never comes from a URL.
use crate::{
    errors::{ApiResult, AppError},
    services::mijia::{MiCloud, MiDevice},
};
use std::collections::HashMap;
use tauri::{AppHandle, Manager, WebviewWindow};

struct PlayerWindow {
    device: MiDevice,
    restore_size: Option<tauri::PhysicalSize<u32>>,
}
#[derive(Default)]
pub struct CameraWindows {
    entries: tokio::sync::Mutex<HashMap<String, PlayerWindow>>,
}
fn window_error() -> AppError {
    AppError::new(
        "CAMERA_WINDOW_FAILED",
        "无法打开或调整观看窗口，请重试",
        true,
    )
}
fn unavailable() -> AppError {
    AppError::new("CAMERA_WINDOW_GONE", "观看窗口已关闭，请重新打开", false)
}
impl CameraWindows {
    pub async fn permits(&self, label: &str, did: &str) -> bool {
        label == "main"
            || self
                .entries
                .lock()
                .await
                .get(label)
                .is_some_and(|p| p.device.did == did)
    }
    pub async fn forget(&self, label: &str) {
        self.entries.lock().await.remove(label);
    }
    pub async fn close_all(&self, app: &AppHandle) {
        let entries = std::mem::take(&mut *self.entries.lock().await);
        for label in entries.keys() {
            if let Some(window) = app.get_webview_window(label) {
                let _ = window.close();
            }
        }
    }
}

#[tauri::command]
pub async fn camera_open_player(
    app: AppHandle,
    service: tauri::State<'_, MiCloud>,
    windows: tauri::State<'_, CameraWindows>,
    did: String,
) -> Result<ApiResult<bool>, ()> {
    let result = async {
        let mut entries = windows.entries.lock().await;
        let snapshot = service.snapshot().await;
        if !snapshot.logged_in {
            return Err(AppError::new(
                "MIJIA_AUTH_EXPIRED",
                "米家登录已失效，请重新扫码",
                false,
            ));
        }
        let device = snapshot
            .devices
            .iter()
            .find(|d| {
                d.did == did
                    && d.camera
                    && d.model == crate::services::mijia_camera::TARGET_MODEL
                    && d.online
            })
            .cloned()
            .ok_or_else(|| AppError::invalid_input("请选择当前账号内在线且支持远程观看的摄像头"))?;
        if let Some((label, _)) = entries.iter().find(|(_, entry)| entry.device.did == did) {
            if let Some(window) = app.get_webview_window(label) {
                let _ = window.unminimize();
                window.show().map_err(|_| window_error())?;
                window.set_focus().map_err(|_| window_error())?;
                return Ok(true);
            }
        }
        entries.retain(|label, _| app.get_webview_window(label).is_some());
        if entries.len() >= 2 {
            return Err(AppError::new(
                "CAMERA_WINDOW_LIMIT",
                "请先关闭一个观看窗口，最多同时打开两路",
                false,
            ));
        }
        service.cameras.stop_device(&did).await;
        let label = format!("camera-{}", uuid::Uuid::new_v4().simple());
        // Register before loading: camera_player_context can be invoked as soon
        // as the webview starts. The lock serializes duplicate opens/logout.
        entries.insert(
            label.clone(),
            PlayerWindow {
                device: device.clone(),
                restore_size: None,
            },
        );
        let built = tauri::WebviewWindowBuilder::new(
            &app,
            &label,
            tauri::WebviewUrl::App("index.html?cameraPlayer=1".into()),
        )
        .title(format!("{} · 米家远程观看", device.name))
        .inner_size(960.0, 680.0)
        .min_inner_size(360.0, 260.0)
        .resizable(true)
        .center()
        .build();
        if built.is_err() {
            entries.remove(&label);
            return Err(window_error());
        }
        Ok(true)
    }
    .await;
    Ok(result.into())
}
#[tauri::command]
pub async fn camera_player_context(
    window: WebviewWindow,
    service: tauri::State<'_, MiCloud>,
    windows: tauri::State<'_, CameraWindows>,
) -> Result<ApiResult<MiDevice>, ()> {
    let result = async {
        let did = windows
            .entries
            .lock()
            .await
            .get(window.label())
            .map(|e| e.device.did.clone())
            .ok_or_else(unavailable)?;
        let snapshot = service.snapshot().await;
        if !snapshot.logged_in {
            return Err(AppError::new(
                "MIJIA_AUTH_EXPIRED",
                "账号已退出，请重新打开观看窗口",
                false,
            ));
        }
        snapshot
            .devices
            .into_iter()
            .find(|d| d.did == did)
            .ok_or_else(unavailable)
    }
    .await;
    Ok(result.into())
}
#[tauri::command]
pub async fn camera_player_mode(
    window: WebviewWindow,
    windows: tauri::State<'_, CameraWindows>,
    mini: bool,
) -> Result<ApiResult<bool>, ()> {
    let result = async {
        let mut entries = windows.entries.lock().await;
        let entry = entries.get_mut(window.label()).ok_or_else(unavailable)?;
        let before = window.inner_size().map_err(|_| window_error())?;
        if mini {
            window
                .set_size(tauri::LogicalSize::new(480.0, 340.0))
                .map_err(|_| window_error())?;
            if window.set_always_on_top(true).is_err() {
                let _ = window.set_size(before);
                return Err(window_error());
            }
            if entry.restore_size.is_none() {
                entry.restore_size = Some(before);
            }
        } else {
            let size = entry
                .restore_size
                .unwrap_or(tauri::PhysicalSize::new(960, 680));
            window.set_size(size).map_err(|_| window_error())?;
            if window.set_always_on_top(false).is_err() {
                let _ = window.set_size(before);
                return Err(window_error());
            }
            entry.restore_size = None;
        }
        Ok(mini)
    }
    .await;
    Ok(result.into())
}

#[tauri::command]
pub async fn camera_player_fullscreen(
    window: WebviewWindow,
    windows: tauri::State<'_, CameraWindows>,
    enabled: bool,
) -> Result<ApiResult<bool>, ()> {
    if !windows.entries.lock().await.contains_key(window.label()) {
        return Ok(Err(unavailable()).into());
    }
    Ok(window
        .set_fullscreen(enabled)
        .map(|_| enabled)
        .map_err(|_| window_error())
        .into())
}
