use crate::{database::Database, errors::AppError, models::AppSettings};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    AppHandle, Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub struct DesktopState {
    pub close_to_tray: AtomicBool,
    pub tray_available: AtomicBool,
    settings_change: tokio::sync::Mutex<()>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopStatus {
    pub tray_available: bool,
    pub auto_start_supported: bool,
    pub auto_start_enabled: Option<bool>,
    pub window_state_enabled: bool,
}
impl DesktopState {
    pub fn new(settings: &AppSettings) -> Self {
        Self {
            close_to_tray: AtomicBool::new(settings.close_to_tray),
            tray_available: AtomicBool::new(false),
            settings_change: tokio::sync::Mutex::new(()),
        }
    }
}
fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        if window.show().is_ok() {
            let _ = window.set_focus();
            let _ = app.emit("app:window_visible", true);
        }
    }
}
pub fn install_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出 Mi Home Desktop", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
    let icon = app
        .default_window_icon()
        .ok_or_else(|| std::io::Error::other("No tray icon"))?;
    TrayIconBuilder::with_id("main-tray")
        .icon(icon.clone())
        .menu(&menu)
        .tooltip("Mi Home Desktop")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show(tray.app_handle());
            }
        })
        .build(app)?;
    app.state::<DesktopState>()
        .tray_available
        .store(true, Ordering::Relaxed);
    Ok(())
}
#[cfg(windows)]
fn startup_enabled(app: &AppHandle) -> Result<bool, AppError> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|_| startup_error())
}
#[cfg(not(windows))]
fn startup_enabled(_app: &AppHandle) -> Result<bool, AppError> {
    Err(startup_error())
}
#[cfg(windows)]
fn set_startup(app: &AppHandle, enabled: bool) -> Result<(), AppError> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|_| startup_error())
}
#[cfg(not(windows))]
fn set_startup(_app: &AppHandle, _enabled: bool) -> Result<(), AppError> {
    Err(startup_error())
}
fn startup_error() -> AppError {
    AppError::new(
        "AUTOSTART_FAILED",
        "无法修改 Windows 开机启动注册，请检查系统权限",
        true,
    )
}
pub fn status(app: &AppHandle) -> DesktopStatus {
    DesktopStatus {
        tray_available: app
            .state::<DesktopState>()
            .tray_available
            .load(Ordering::Relaxed),
        auto_start_supported: cfg!(windows),
        auto_start_enabled: startup_enabled(app).ok(),
        window_state_enabled: true,
    }
}
pub async fn save_settings(
    app: &AppHandle,
    state: &DesktopState,
    database: &Database,
    settings: AppSettings,
) -> Result<AppSettings, AppError> {
    let _mutation = state.settings_change.lock().await;
    settings.validate()?;
    let previous = database.settings().await?;
    let registration_before = if settings.auto_start != previous.auto_start {
        let before = startup_enabled(app)?;
        set_startup(app, settings.auto_start)?;
        Some(before)
    } else {
        None
    };
    match database.save_settings(settings).await {
        Ok(settings) => {
            state
                .close_to_tray
                .store(settings.close_to_tray, Ordering::Relaxed);
            Ok(settings)
        }
        Err(error) => {
            if let Some(before) = registration_before {
                if set_startup(app, before).is_err() {
                    tracing::warn!("Unable to restore startup preference");
                    return Err(AppError::new(
                        "AUTOSTART_ROLLBACK_FAILED",
                        "设置保存失败，且无法还原系统启动注册；请在 Windows 启动应用设置中检查",
                        false,
                    ));
                }
            }
            Err(error)
        }
    }
}
pub fn window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label().starts_with("camera-") {
        if matches!(
            event,
            tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
        ) {
            let app = window.app_handle().clone();
            let label = window.label().to_owned();
            tauri::async_runtime::spawn(async move {
                app.state::<crate::services::mijia::MiCloud>()
                    .cameras
                    .stop_owner(&label)
                    .await;
                app.state::<crate::camera_windows::CameraWindows>()
                    .forget(&label)
                    .await;
            });
        }
        return;
    }
    if window.label() != "main" {
        return;
    }
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if let Some(cloud) = window.try_state::<crate::services::mijia::MiCloud>() {
            let cameras = cloud.cameras.clone();
            tauri::async_runtime::spawn(async move {
                cameras.stop_owner("main").await;
            });
        }
        let Some(state) = window.try_state::<DesktopState>() else {
            return;
        };
        if state.close_to_tray.load(Ordering::Relaxed)
            && state.tray_available.load(Ordering::Relaxed)
        {
            if window.hide().is_ok() {
                api.prevent_close();
                let _ = window.emit("app:window_visible", false);
            } else {
                tracing::warn!("Unable to hide main window");
            }
        } else {
            window.app_handle().exit(0);
        }
    }
}
