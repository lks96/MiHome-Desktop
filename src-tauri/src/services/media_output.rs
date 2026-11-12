//! Session-local FFmpeg pipeline and authenticated loopback HLS output.
use super::cs2::failure;
use crate::errors::AppError;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::{Child, ChildStdin, Command},
    sync::Semaphore,
    task::{JoinHandle, JoinSet},
};

pub fn engine_path() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or(Path::new("."))
            .join("ffmpeg/bin/ffmpeg.exe")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/usr/bin/ffmpeg")
    }
}
pub struct MediaOutput {
    child: Child,
    input: Option<ChildStdin>,
    server: JoinHandle<()>,
    directory: TempDir,
    pub url: String,
}
impl Drop for MediaOutput {
    fn drop(&mut self) {
        self.server.abort();
        let _ = self.child.start_kill();
    }
}
impl MediaOutput {
    pub async fn start(codec: u32) -> Result<Self, AppError> {
        if !matches!(codec, 4 | 5) {
            return Err(failure("VIDEO_CODEC_UNSUPPORTED", "摄像头视频编码暂不支持"));
        }
        let directory = tempfile::Builder::new()
            .prefix("mihome-stream-")
            .tempdir()
            .map_err(|_| failure("VIDEO_CACHE_FAILED", "无法创建临时视频缓存"))?;
        let engine = engine_path();
        if !engine.is_file() {
            return Err(failure(
                "VIDEO_ENGINE_MISSING",
                "视频组件缺失，请重新安装完整客户端",
            ));
        }
        let mut cmd = Command::new(engine);
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostats",
            "-nostdin",
            "-fflags",
            "+genpts",
            "-r",
            "20",
            "-probesize",
            "262144",
            "-analyzeduration",
            "1000000",
            "-f",
            if codec == 4 { "h264" } else { "hevc" },
            "-i",
            "pipe:0",
            "-an",
        ]);
        if codec == 4 {
            cmd.args(["-c:v", "copy"]);
        } else {
            #[cfg(windows)]
            cmd.args([
                "-c:v",
                "h264_mf",
                "-hw_encoding",
                "0",
                "-b:v",
                "2500k",
                "-g",
                "30",
                "-pix_fmt",
                "yuv420p",
            ]);
            #[cfg(not(windows))]
            cmd.args([
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-tune",
                "zerolatency",
                "-b:v",
                "2500k",
                "-g",
                "30",
                "-pix_fmt",
                "yuv420p",
            ]);
        }
        cmd.args([
            "-f",
            "hls",
            "-hls_time",
            "1",
            "-hls_list_size",
            "5",
            "-hls_flags",
            "delete_segments+independent_segments+temp_file",
            "-hls_segment_filename",
        ])
        .arg(directory.path().join("seg%06d.ts"))
        .arg(directory.path().join("index.m3u8"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let mut child = cmd
            .spawn()
            .map_err(|_| failure("VIDEO_ENGINE_FAILED", "无法启动视频组件，请重新安装客户端"))?;
        let input = child.stdin.take();
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| failure("VIDEO_LOCAL_SERVER_FAILED", "无法打开本机视频端口"))?;
        let port = listener
            .local_addr()
            .map_err(|_| failure("VIDEO_LOCAL_SERVER_FAILED", "无法读取本机视频端口"))?
            .port();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let url = format!("http://127.0.0.1:{port}/{token}/index.m3u8");
        let root = directory.path().to_owned();
        let server = tokio::spawn(serve(listener, root, token));
        Ok(Self {
            child,
            input,
            server,
            directory,
            url,
        })
    }
    pub async fn write(&mut self, bytes: &[u8]) -> Result<(), AppError> {
        if self
            .child
            .try_wait()
            .map_err(|_| failure("VIDEO_ENGINE_FAILED", "视频处理组件状态异常"))?
            .is_some()
        {
            return Err(failure(
                "VIDEO_DECODE_FAILED",
                "视频处理失败；H.265 转换需要 Windows Media Foundation H.264 编码器",
            ));
        }
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| failure("VIDEO_ENGINE_FAILED", "视频输入已关闭"))?;
        tokio::time::timeout(Duration::from_secs(3), input.write_all(bytes))
            .await
            .map_err(|_| failure("VIDEO_ENGINE_TIMEOUT", "视频处理超时，请重试"))?
            .map_err(|_| failure("VIDEO_DECODE_FAILED", "无法处理摄像头视频编码"))
    }
    pub async fn ready(&self) -> bool {
        let Ok(data) = tokio::fs::read(self.directory.path().join("index.m3u8")).await else {
            return false;
        };
        data.windows(7).any(|b| b == b"#EXTINF")
    }
    pub async fn close(&mut self) {
        self.server.abort();
        self.input.take();
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}
fn allowed_origin(value: &str) -> bool {
    matches!(
        value,
        "http://tauri.localhost" | "https://tauri.localhost" | "tauri://localhost"
    ) || (cfg!(debug_assertions)
        && matches!(value, "http://localhost:1420" | "http://127.0.0.1:1420"))
}
fn media_name(path: &str, token: &str) -> Option<String> {
    let rest = path
        .strip_prefix('/')?
        .strip_prefix(token)?
        .strip_prefix('/')?;
    if rest == "index.m3u8" {
        return Some(rest.into());
    }
    let digits = rest.strip_prefix("seg")?.strip_suffix(".ts")?;
    (digits.len() >= 6 && digits.len() <= 10 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| rest.to_owned())
}
async fn serve(listener: TcpListener, root: PathBuf, token: String) {
    let permits = Arc::new(Semaphore::new(12));
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            accepted=listener.accept()=> {
                let Ok((socket,peer))=accepted else {break};if !peer.ip().is_loopback() {continue;}
                let Ok(permit)=permits.clone().try_acquire_owned() else {continue;};
                let root=root.clone();let token=token.clone();
                clients.spawn(async move {let _permit=permit;let _=tokio::time::timeout(Duration::from_secs(5),respond(socket,&root,&token)).await;});
            }
            _=clients.join_next(), if !clients.is_empty()=>{}
        }
    }
}
async fn respond(mut socket: TcpStream, root: &Path, token: &str) -> std::io::Result<()> {
    let mut header = Vec::with_capacity(1024);
    let mut buf = [0; 1024];
    while !header.windows(4).any(|b| b == b"\r\n\r\n") {
        let n = socket.read(&mut buf).await?;
        if n == 0 || header.len() + n > 8192 {
            return Ok(());
        }
        header.extend(&buf[..n]);
    }
    let Ok(text) = std::str::from_utf8(&header) else {
        return Ok(());
    };
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let method = parts.next();
    let path = parts.next().unwrap_or_default();
    let mut origin = None;
    let mut host = None;
    let mut bad = false;
    for line in lines.filter(|l| !l.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            bad = true;
            break;
        };
        if name.eq_ignore_ascii_case("Origin") {
            if origin.is_some() {
                bad = true;
            }
            origin = Some(value.trim());
        }
        if name.eq_ignore_ascii_case("Host") {
            if host.is_some() {
                bad = true;
            }
            host = Some(value.trim());
        }
    }
    let expected_host = format!("127.0.0.1:{}", socket.local_addr()?.port());
    let file = media_name(path, token);
    let valid = !bad
        && method == Some("GET")
        && host == Some(expected_host.as_str())
        && origin.is_none_or(allowed_origin)
        && file.is_some();
    let mut status = "403 Forbidden";
    let mut data = Vec::new();
    let mut mime = "text/plain";
    if valid {
        let name = file.unwrap();
        if let Ok(meta) = tokio::fs::metadata(root.join(&name)).await {
            if meta.is_file() && meta.len() <= 16 * 1024 * 1024 {
                if let Ok(bytes) = tokio::fs::read(root.join(&name)).await {
                    data = bytes;
                    status = "200 OK";
                    mime = if name.ends_with("m3u8") {
                        "application/vnd.apple.mpegurl"
                    } else {
                        "video/mp2t"
                    };
                }
            }
        }
        if status != "200 OK" {
            status = "404 Not Found";
        }
    }
    let cors = origin
        .filter(|v| allowed_origin(v))
        .map(|v| format!("Access-Control-Allow-Origin: {v}\r\nVary: Origin\r\n"))
        .unwrap_or_default();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n{cors}\r\n",
        data.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.write_all(&data).await?;
    socket.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn media_paths_and_origins_do_not_allow_traversal_or_network_clients() {
        for value in [
            "/secret/../x",
            "/secret/%2e%2e/index.m3u8",
            "/wrong/index.m3u8",
            "/secret/seg000001.ts?x=1",
            "/secret/seg000001.ts/xx",
        ] {
            assert!(media_name(value, "secret").is_none());
        }
        assert_eq!(
            media_name("/secret/seg000001.ts", "secret"),
            Some("seg000001.ts".into())
        );
        assert!(!allowed_origin("https://tauri.localhost.evil.example"));
        assert!(!allowed_origin("null"));
    }
    #[cfg(not(windows))]
    #[tokio::test]
    async fn generated_h264_and_hevc_produce_decodable_hls_only_for_the_session_origin() {
        for (codec, encoder, format) in [(4, "libx264", "h264"), (5, "libx265", "hevc")] {
            let sample = Command::new(engine_path())
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=160x96:rate=20",
                    "-t",
                    "4",
                    "-c:v",
                    encoder,
                    "-threads",
                    "1",
                    "-preset",
                    "ultrafast",
                    "-g",
                    "20",
                    "-f",
                    format,
                    "pipe:1",
                ])
                .stderr(Stdio::null())
                .output()
                .await
                .unwrap();
            assert!(sample.status.success());
            assert!(!sample.stdout.is_empty());
            let mut out = MediaOutput::start(codec).await.unwrap();
            out.write(&sample.stdout).await.unwrap();
            tokio::time::timeout(Duration::from_secs(10), async {
                while !out.ready().await {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await
            .unwrap();
            let http = reqwest::Client::builder().no_proxy().build().unwrap();
            let playlist = http
                .get(&out.url)
                .header("Origin", "http://tauri.localhost")
                .send()
                .await
                .unwrap();
            assert_eq!(playlist.status(), 200);
            let content = playlist.text().await.unwrap();
            let segment = content
                .lines()
                .find(|l| l.starts_with("seg") && l.ends_with(".ts"))
                .unwrap();
            let segment_url = out.url.replace("index.m3u8", segment);
            let segment_bytes = http
                .get(segment_url)
                .header("Origin", "http://tauri.localhost")
                .send()
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap();
            assert!(segment_bytes.len() > 188);
            let mut decoder = Command::new(engine_path())
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-i",
                    "pipe:0",
                    "-frames:v",
                    "1",
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    "rgb24",
                    "pipe:1",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut input = decoder.stdin.take().unwrap();
            let data = segment_bytes.to_vec();
            let writer = tokio::spawn(async move {
                let _ = input.write_all(&data).await;
            });
            let decoded = decoder.wait_with_output().await.unwrap();
            writer.await.unwrap();
            assert!(decoded.status.success());
            assert_eq!(decoded.stdout.len(), 160 * 96 * 3);
            assert_eq!(
                http.get(&out.url)
                    .header("Origin", "https://untrusted.example")
                    .send()
                    .await
                    .unwrap()
                    .status(),
                403
            );
            assert_eq!(
                http.get(out.url.replace("index.m3u8", "../index.m3u8"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                403
            );
            let address = out.url.clone();
            out.close().await;
            drop(out);
            assert!(http.get(address).send().await.is_err());
        }
    }
}
