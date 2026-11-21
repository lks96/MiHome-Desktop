#[cfg(feature = "desktop")]
mod camera_windows;
#[cfg(feature = "desktop")]
mod commands;
pub mod database;
#[cfg(feature = "desktop")]
mod desktop;
pub mod errors;
pub mod logging;
#[cfg(feature = "desktop")]
mod mijia_commands;
pub mod models;
pub mod security;
pub mod services;

#[cfg(feature = "desktop")]
pub fn run() {
    use services::session::{HaEvent, HaService};
    use std::sync::Arc;
    use tauri::{Emitter, Manager};
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    tauri::Builder::default()
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("Mi Home Desktop")
                .build(),
        )
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(desktop::window_event)
        .setup(|app| {
            let initialize = || -> Result<(), &'static str> {
                match app
                    .path()
                    .app_log_dir()
                    .ok()
                    .and_then(|path| logging::init(&path).ok())
                {
                    Some(guard) => {
                        app.manage(guard);
                    }
                    None => eprintln!("Unable to initialize application logging"),
                }
                let data_dir = app
                    .path()
                    .app_data_dir()
                    .map_err(|_| "无法定位本地数据目录")?;
                std::fs::create_dir_all(&data_dir).map_err(|_| "无法创建本地数据目录")?;
                let database = tauri::async_runtime::block_on(database::Database::open(
                    &data_dir.join("mihome.sqlite"),
                ))
                .map_err(|_| "无法打开或升级本地数据库")?;
                let settings =
                    tauri::async_runtime::block_on(database.settings()).unwrap_or_else(|error| {
                        tracing::warn!(
                            code = error.code,
                            "Unable to load stored preferences at startup"
                        );
                        models::AppSettings {
                            auto_connect: false,
                            ..models::AppSettings::default()
                        }
                    });
                app.manage(desktop::DesktopState::new(&settings));
                let handle = app.handle().clone();
                let service = HaService::new(
                    Arc::new(security::SystemCredentials),
                    Arc::new(move |event| {
                        let result = match event {
                            HaEvent::Connection(status) => {
                                handle.emit("ha:connection_changed", status)
                            }
                            HaEvent::Synced(entities) => handle.emit("ha:sync_completed", entities),
                            HaEvent::Changed(entity) => handle.emit("device:state_changed", entity),
                            HaEvent::Removed(id) => handle.emit("device:removed", id),
                            HaEvent::Catalog(catalog) => handle.emit("ha:catalog_changed", catalog),
                            HaEvent::SyncStarted => handle.emit("ha:sync_started", ()),
                        };
                        if result.is_err() {
                            tracing::warn!("Unable to deliver Home Assistant UI event");
                        }
                    }),
                );
                app.manage(services::cameras::CameraSessions::default());
                app.manage(camera_windows::CameraWindows::default());
                app.manage(database.clone());
                app.manage(service.clone());
                app.manage(services::mijia::MiCloud::new(Arc::new(
                    security::SystemCredentials,
                )));

                // Create the WebView only after its IPC state is ready.
                let config = app
                    .config()
                    .app
                    .windows
                    .iter()
                    .find(|window| window.label == "main")
                    .ok_or("找不到主窗口配置")?;
                tauri::WebviewWindowBuilder::from_config(app.handle(), config)
                    .and_then(|builder| builder.build())
                    .map_err(|_| "无法创建主窗口，请检查 Microsoft Edge WebView2 Runtime")?;
                if desktop::install_tray(app.handle()).is_err() {
                    tracing::warn!("System tray unavailable");
                }
                if settings.auto_connect {
                    tauri::async_runtime::spawn(async move {
                        let result = async {
                            if database.connection().await?.is_some() {
                                service.connect(&database).await?;
                            }
                            Ok::<(), errors::AppError>(())
                        }
                        .await;
                        if let Err(error) = result {
                            service.report_error(error).await;
                        }
                    });
                }
                tracing::info!(
                    version = env!("CARGO_PKG_VERSION"),
                    "Application initialized"
                );
                Ok(())
            };
            if let Err(stage) = initialize() {
                tracing::error!(stage, "Application startup failed");
                let handle = app.handle().clone();
                app.dialog()
                    .message(format!("Mi Home Desktop 启动失败。\n{stage}"))
                    .kind(MessageDialogKind::Error)
                    .show(move |_| handle.exit(1));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            mijia_commands::mijia_restore,
            mijia_commands::mijia_start_login,
            mijia_commands::mijia_poll_login,
            mijia_commands::mijia_cancel_login,
            mijia_commands::mijia_logout,
            mijia_commands::mijia_sync,
            mijia_commands::mijia_get_properties,
            mijia_commands::mijia_camera_probe,
            mijia_commands::camera_remote_start,
            mijia_commands::camera_remote_status,
            mijia_commands::camera_remote_stop,
            mijia_commands::camera_remote_control,
            mijia_commands::camera_remote_audio,
            mijia_commands::camera_remote_speaker,
            mijia_commands::camera_remote_ptz,
            camera_windows::camera_open_player,
            camera_windows::camera_player_context,
            camera_windows::camera_player_mode,
            camera_windows::camera_player_fullscreen,
            mijia_commands::mijia_set_property,
            commands::settings_get,
            commands::settings_update,
            commands::settings_get_app_info,
            commands::ha_test_connection,
            commands::ha_save_connection,
            commands::ha_get_saved_connection,
            commands::ha_get_connection_status,
            commands::ha_connect,
            commands::ha_disconnect,
            commands::ha_get_entities,
            commands::ha_get_catalog,
            commands::device_get_preferences,
            commands::device_update_preferences,
            commands::device_call_service,
            commands::camera_get_configuration,
            commands::camera_save_gateway,
            commands::camera_probe_gateway,
            commands::camera_save_profile,
            commands::camera_get_snapshot,
            commands::camera_start_session,
            commands::camera_exchange_sdp,
            commands::camera_keep_session,
            commands::camera_stop_session,
            commands::scene_get_list,
            commands::scene_get_favorites,
            commands::scene_set_favorite,
            commands::scene_execute,
            commands::desktop_get_status,
            commands::settings_get_diagnostics,
            commands::settings_export_logs
        ])
        .build(tauri::generate_context!())
        .expect("Unable to initialize Mi Home Desktop; check application logs")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
                if let Some(cloud) = app.try_state::<services::mijia::MiCloud>() {
                    tauri::async_runtime::block_on(cloud.cameras.stop_all());
                }
            }
        });
}
