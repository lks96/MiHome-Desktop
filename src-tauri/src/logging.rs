use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;

pub fn init(path: &Path) -> Result<WorkerGuard, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(path)?;
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("mihome")
        .filename_suffix("log")
        .max_log_files(7)
        .build(path)?;
    let (writer, guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_env_filter("mi_home_desktop_lib=info,sqlx=warn")
        .with_writer(writer)
        .try_init()
        .map_err(std::io::Error::other)?;
    Ok(guard)
}
