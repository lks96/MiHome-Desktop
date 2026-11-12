use super::{
    cs2::{failure, relay},
    media_output::MediaOutput,
    mijia_camera::RemoteGrant,
    miss,
};
use crate::errors::AppError;
use serde::Serialize;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::watch, task::JoinHandle};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamStatus {
    pub session_id: String,
    pub stage: &'static str,
    pub failed_stage: Option<&'static str>,
    pub elapsed_ms: u64,
    pub attempt: u32,
    pub media_packets: u64,
    pub keyframes: u64,
    pub codec: Option<&'static str>,
    pub playback_url: Option<String>,
    pub error: Option<AppError>,
}
struct Entry {
    did: String,
    status: Arc<Mutex<StreamStatus>>,
    born: Instant,
    lease: Arc<Mutex<Instant>>,
    cancel: watch::Sender<bool>,
    task: JoinHandle<()>,
}
#[derive(Clone, Default)]
pub struct CameraStreams {
    entries: Arc<tokio::sync::Mutex<HashMap<String, Entry>>>,
}
impl CameraStreams {
    pub async fn launch<F>(&self, did: &str, provision: F) -> Result<StreamStatus, AppError>
    where
        F: Future<Output = Result<RemoteGrant, AppError>> + Send + 'static,
    {
        let mut entries = self.entries.lock().await;
        // Reap finished tasks before deciding the resource budget.
        entries.retain(|_, e| !e.task.is_finished());
        if entries.values().any(|e| e.did == did) {
            return Err(failure(
                "CAMERA_ALREADY_OPEN",
                "此摄像头已有播放会话，请先停止",
            ));
        }
        if entries.len() >= 2 {
            return Err(failure("CAMERA_SESSION_LIMIT", "请先关闭其他摄像头播放"));
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let initial = StreamStatus {
            session_id: id.clone(),
            stage: "provisioning",
            failed_stage: None,
            elapsed_ms: 0,
            attempt: 1,
            media_packets: 0,
            keyframes: 0,
            codec: None,
            playback_url: None,
            error: None,
        };
        let status = Arc::new(Mutex::new(initial.clone()));
        let state = status.clone();
        let born = Instant::now();
        let lease = Arc::new(Mutex::new(born));
        let heartbeat = lease.clone();
        let (cancel, mut cancelled) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut link = None;
            let mut output = None;
            let mut stop_message = None;
            let outcome = tokio::select! {
                result=run(provision,&state,&mut link,&mut output,&mut stop_message)=>result,
                _=cancelled.changed()=>Ok(()),
                _=async {loop {tokio::time::sleep(Duration::from_secs(5)).await;if heartbeat.lock().unwrap().elapsed()>Duration::from_secs(40){break;}}}=>Err(failure("CAMERA_LEASE_EXPIRED","播放页面已关闭，视频会话已释放")),
            };
            state.lock().unwrap().playback_url = None;
            if let Some(mut connection) = link {
                if let Some(stop) = stop_message {
                    let _ =
                        tokio::time::timeout(Duration::from_millis(300), connection.command(&stop))
                            .await;
                }
                connection.close().await;
            }
            if let Some(mut pipeline) = output {
                pipeline.close().await;
            }
            let mut status = state.lock().unwrap();
            match outcome {
                Err(error) => {
                    status.failed_stage = Some(status.stage);
                    status.stage = "failed";
                    status.error = Some(error);
                }
                Ok(()) => {
                    status.stage = "stopped";
                }
            }
        });
        entries.insert(
            id,
            Entry {
                did: did.into(),
                status,
                born,
                lease,
                cancel,
                task,
            },
        );
        Ok(initial)
    }
    pub async fn status(&self, id: &str) -> Result<StreamStatus, AppError> {
        let entries = self.entries.lock().await;
        let entry = entries
            .get(id)
            .ok_or_else(|| failure("CAMERA_SESSION_GONE", "视频会话已结束，请重新连接"))?;
        *entry.lease.lock().unwrap() = Instant::now();
        let mut status = entry.status.lock().unwrap().clone();
        status.elapsed_ms = entry.born.elapsed().as_millis() as u64;
        Ok(status)
    }
    pub async fn stop(&self, id: &str) {
        let entry = self.entries.lock().await.remove(id);
        if let Some(mut entry) = entry {
            let _ = entry.cancel.send(true);
            if tokio::time::timeout(Duration::from_secs(5), &mut entry.task)
                .await
                .is_err()
            {
                entry.task.abort();
                let _ = entry.task.await;
            }
        }
    }
    pub async fn stop_all(&self) {
        let entries = std::mem::take(&mut *self.entries.lock().await);
        for entry in entries.values() {
            let _ = entry.cancel.send(true);
        }
        for (_, mut entry) in entries {
            if tokio::time::timeout(Duration::from_secs(5), &mut entry.task)
                .await
                .is_err()
            {
                entry.task.abort();
                let _ = entry.task.await;
            }
        }
    }
}

async fn run<F>(
    provision: F,
    status: &Arc<Mutex<StreamStatus>>,
    connection: &mut Option<relay::Connection>,
    pipeline: &mut Option<MediaOutput>,
    stop_message: &mut Option<zeroize::Zeroizing<Vec<u8>>>,
) -> Result<(), AppError>
where
    F: Future<Output = Result<RemoteGrant, AppError>>,
{
    let grant = tokio::time::timeout(Duration::from_secs(25), provision)
        .await
        .map_err(|_| failure("CAMERA_PROVISION_TIMEOUT", "取得摄像头连接参数超时"))??;
    let st = status.clone();
    let stage: relay::Stage = Arc::new(move |value| {
        st.lock().unwrap().stage = value;
    });
    *connection = Some(relay::dial(&grant.peer, &grant.servers, stage).await?);
    let conn = connection.as_mut().unwrap();
    status.lock().unwrap().stage = "miss_authenticating";
    conn.command(&miss::auth_message(&grant.public, &grant.sign))
        .await?;
    let auth_end = tokio::time::Instant::now() + Duration::from_secs(12);
    loop {
        let msg = tokio::time::timeout_at(auth_end, conn.next())
            .await
            .map_err(|_| failure("MISS_AUTH_TIMEOUT", "摄像头视频认证超时"))??;
        if msg.channel == 0 && miss::authenticated(&msg.bytes)? {
            break;
        }
    }
    status.lock().unwrap().stage = "miss_authenticated";
    *stop_message = Some(zeroize::Zeroizing::new(miss::encrypted_command(
        &grant.key,
        0x103,
        &[],
    )));
    conn.command(&miss::encrypted_command(
        &grant.key,
        0x102,
        br#"{"videoquality":2,"enableaudio":0}"#,
    ))
    .await?;
    status.lock().unwrap().stage = "waiting_media";
    let mut received = Instant::now();
    let mut first = None;
    let mut codec = None;
    let mut primed = false;
    let mut ready = false;
    let mut headers = zeroize::Zeroizing::new(Vec::new());
    loop {
        let message = tokio::time::timeout(Duration::from_secs(15), conn.next())
            .await
            .map_err(|_| failure("MISS_MEDIA_TIMEOUT", "摄像头未返回视频数据"))??;
        if received.elapsed() > Duration::from_secs(20) {
            return Err(failure("MISS_MEDIA_TIMEOUT", "摄像头视频数据中断"));
        }
        if message.channel != 2 {
            continue;
        }
        let Some(video) = miss::video(&grant.key, &message.bytes)? else {
            continue;
        };
        received = Instant::now();
        first.get_or_insert(received);
        if !ready && first.is_some_and(|t| t.elapsed() > Duration::from_secs(30)) {
            return Err(failure(
                "VIDEO_DECODE_TIMEOUT",
                "已收到视频，但尚未取得可解码画面",
            ));
        }
        {
            let mut s = status.lock().unwrap();
            s.media_packets += 1;
            s.keyframes += u64::from(video.keyframe);
            s.codec = Some(if video.codec == 4 { "H.264" } else { "H.265" });
        }
        // Start at a complete random-access point; don't feed an undecodable delta stream.
        if !primed {
            let params = miss::parameter_sets(video.codec, &video.bytes);
            if headers.len() + params.len() <= 65536 {
                headers.extend(&*params);
            }
        }
        if !primed && !video.keyframe {
            continue;
        }
        primed = true;
        if codec.is_some_and(|v| v != video.codec) {
            return Err(failure(
                "VIDEO_CODEC_CHANGED",
                "摄像头视频格式已改变，请重新连接",
            ));
        }
        codec = Some(video.codec);
        if pipeline.is_none() {
            status.lock().unwrap().stage = "preparing_video";
            *pipeline = Some(MediaOutput::start(video.codec).await?);
        }
        let output = pipeline.as_mut().unwrap();
        if !headers.is_empty() {
            output.write(&headers).await?;
            headers.clear();
        }
        output.write(&video.bytes).await?;
        if !ready && output.ready().await {
            ready = true;
            let mut s = status.lock().unwrap();
            s.stage = "media_ready";
            s.playback_url = Some(output.url.clone());
        }
        if !ready && first.is_some_and(|t| t.elapsed() > Duration::from_secs(30)) {
            return Err(failure(
                "VIDEO_DECODE_TIMEOUT",
                "已收到视频，但无法生成可播放画面",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn stop_cancels_provisioning_idempotently_and_keeps_sessions_private() {
        let sessions = CameraStreams::default();
        let status = sessions
            .launch("private-device", std::future::pending())
            .await
            .unwrap();
        assert!(
            !serde_json::to_string(&status)
                .unwrap()
                .contains("private-device")
        );
        assert!(
            sessions
                .launch("private-device", std::future::pending())
                .await
                .is_err()
        );
        tokio::time::timeout(Duration::from_secs(1), sessions.stop(&status.session_id))
            .await
            .unwrap();
        assert!(sessions.status(&status.session_id).await.is_err());
        sessions.stop(&status.session_id).await;
    }
}
