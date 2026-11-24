use super::{
    cs2::{failure, relay},
    media_output::MediaOutput,
    mijia_camera::RemoteGrant,
    miss,
};
use crate::errors::AppError;
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};

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
    pub auth_diagnostics: miss::AuthDiagnostics,
    pub transport_diagnostics: Option<relay::TransportDiagnostics>,
    pub timing_diagnostics: Option<super::media_timing::TimingDiagnostics>,
    pub requested_quality: u8,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaControl {
    ListenOn,
    ListenOff,
    TalkOn,
    TalkOff,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraDirection {
    Left = 1,
    Right = 2,
    Up = 3,
    Down = 4,
}

pub enum ControlAction {
    Media(MediaControl),
    Move(CameraDirection),
}

struct ControlRequest {
    action: ControlAction,
    reply: oneshot::Sender<Result<(), AppError>>,
}

type AudioQueue = Arc<Mutex<VecDeque<miss::AudioPacket>>>;

struct Entry {
    did: String,
    owner: String,
    status: Arc<Mutex<StreamStatus>>,
    born: Instant,
    lease: Arc<Mutex<Instant>>,
    cancel: watch::Sender<bool>,
    task: JoinHandle<()>,
    controls: mpsc::Sender<ControlRequest>,
    speaker: mpsc::Sender<zeroize::Zeroizing<Vec<u8>>>,
    audio: AudioQueue,
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
        self.launch_owned(did, "main", 2, provision).await
    }
    pub async fn launch_owned<F>(
        &self,
        did: &str,
        owner: &str,
        quality: u8,
        provision: F,
    ) -> Result<StreamStatus, AppError>
    where
        F: Future<Output = Result<RemoteGrant, AppError>> + Send + 'static,
    {
        if quality > 3 {
            return Err(AppError::invalid_input("请选择自动、流畅、高清或超清画质"));
        }
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
            auth_diagnostics: miss::AuthDiagnostics::default(),
            transport_diagnostics: None,
            timing_diagnostics: None,
            requested_quality: quality,
        };
        let status = Arc::new(Mutex::new(initial.clone()));
        let state = status.clone();
        let born = Instant::now();
        let lease = Arc::new(Mutex::new(born));
        let heartbeat = lease.clone();
        let (cancel, mut cancelled) = watch::channel(false);
        let (controls, control_rx) = mpsc::channel(4);
        let (speaker, speaker_rx) = mpsc::channel(4);
        let audio = AudioQueue::default();
        let media = MediaControls {
            control_rx,
            speaker_rx,
            audio: audio.clone(),
        };
        let task = tokio::spawn(async move {
            let mut link = None;
            let mut output = None;
            let mut stop_message = None;
            let mut speaker_stop = None;
            let outcome = tokio::select! {
                result=run(provision,&state,&mut link,&mut output,&mut stop_message,quality,media,&mut speaker_stop)=>result,
                _=cancelled.changed()=>Ok(()),
                _=async {loop {tokio::time::sleep(Duration::from_secs(5)).await;if heartbeat.lock().unwrap().elapsed()>Duration::from_secs(40){break;}}}=>Err(failure("CAMERA_LEASE_EXPIRED","播放页面已关闭，视频会话已释放")),
            };
            state.lock().unwrap().playback_url = None;
            if let Some(mut connection) = link {
                // Stop the camera speaker before tearing down the media session.
                if let Some(stop) = speaker_stop {
                    let _ =
                        tokio::time::timeout(Duration::from_millis(300), connection.command(&stop))
                            .await;
                }
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
                owner: owner.into(),
                status,
                born,
                lease,
                cancel,
                task,
                controls,
                speaker,
                audio,
            },
        );
        Ok(initial)
    }
    pub async fn status(&self, id: &str) -> Result<StreamStatus, AppError> {
        self.status_owned(id, "main").await
    }
    pub async fn status_owned(&self, id: &str, owner: &str) -> Result<StreamStatus, AppError> {
        let entries = self.entries.lock().await;
        let entry = entries
            .get(id)
            .filter(|e| e.owner == owner)
            .ok_or_else(|| failure("CAMERA_SESSION_GONE", "视频会话已结束，请重新连接"))?;
        *entry.lease.lock().unwrap() = Instant::now();
        let mut status = entry.status.lock().unwrap().clone();
        status.elapsed_ms = entry.born.elapsed().as_millis() as u64;
        Ok(status)
    }
    pub async fn control(
        &self,
        id: &str,
        owner: &str,
        action: ControlAction,
    ) -> Result<bool, AppError> {
        let controls = {
            let entries = self.entries.lock().await;
            let entry = entries
                .get(id)
                .filter(|entry| entry.owner == owner)
                .ok_or_else(session_gone)?;
            if entry.status.lock().unwrap().stage != "media_ready" {
                return Err(failure("CAMERA_NOT_READY", "请等待摄像头开始播放"));
            }
            entry.controls.clone()
        };
        let timeout = match action {
            ControlAction::Media(_) => 3,
            ControlAction::Move(_) => 6,
        };
        let (reply, result) = oneshot::channel();
        controls
            .try_send(ControlRequest { action, reply })
            .map_err(|_| failure("CAMERA_CONTROL_BUSY", "摄像头控制暂时繁忙，请重试"))?;
        tokio::time::timeout(Duration::from_secs(timeout), result)
            .await
            .map_err(|_| failure("CAMERA_CONTROL_TIMEOUT", "摄像头控制超时"))?
            .map_err(|_| session_gone())??;
        Ok(true)
    }

    pub async fn audio(&self, id: &str, owner: &str) -> Result<Vec<miss::AudioPacket>, AppError> {
        let entries = self.entries.lock().await;
        let entry = entries
            .get(id)
            .filter(|entry| entry.owner == owner)
            .ok_or_else(session_gone)?;
        Ok(entry.audio.lock().unwrap().drain(..).collect())
    }

    pub async fn speaker(&self, id: &str, owner: &str, samples: Vec<u8>) -> Result<bool, AppError> {
        if samples.len() != 320 {
            return Err(AppError::invalid_input("对讲音频帧长度错误"));
        }
        let entries = self.entries.lock().await;
        let entry = entries
            .get(id)
            .filter(|entry| entry.owner == owner)
            .ok_or_else(session_gone)?;
        Ok(entry
            .speaker
            .try_send(zeroize::Zeroizing::new(samples))
            .is_ok())
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
    pub async fn stop_owned(&self, id: &str, owner: &str) {
        let entry = {
            let mut entries = self.entries.lock().await;
            if entries.get(id).is_some_and(|e| e.owner == owner) {
                entries.remove(id)
            } else {
                None
            }
        };
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
    pub async fn stop_owner(&self, owner: &str) {
        let ids = self
            .entries
            .lock()
            .await
            .iter()
            .filter(|(_, e)| e.owner == owner)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            self.stop_owned(&id, owner).await;
        }
    }
    pub async fn stop_device(&self, did: &str) {
        let ids = self
            .entries
            .lock()
            .await
            .iter()
            .filter(|(_, e)| e.did == did)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            self.stop(&id).await;
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

fn session_gone() -> AppError {
    failure("CAMERA_SESSION_GONE", "视频会话已结束，请重新连接")
}

struct MediaControls {
    control_rx: mpsc::Receiver<ControlRequest>,
    speaker_rx: mpsc::Receiver<zeroize::Zeroizing<Vec<u8>>>,
    audio: AudioQueue,
}

struct PendingMove {
    deadline: tokio::time::Instant,
    reply: oneshot::Sender<Result<(), AppError>>,
}

async fn run<F>(
    provision: F,
    status: &Arc<Mutex<StreamStatus>>,
    connection: &mut Option<relay::Connection>,
    pipeline: &mut Option<MediaOutput>,
    stop_message: &mut Option<zeroize::Zeroizing<Vec<u8>>>,
    quality: u8,
    mut media: MediaControls,
    speaker_stop: &mut Option<zeroize::Zeroizing<Vec<u8>>>,
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
    {
        let mut s = status.lock().unwrap();
        s.stage = "miss_authenticating";
        s.transport_diagnostics = Some(conn.diagnostics());
    }
    conn.command(&miss::auth_message(&grant.public, &grant.sign))
        .await?;
    let auth_end = tokio::time::Instant::now() + Duration::from_secs(12);
    loop {
        let result = tokio::time::timeout_at(auth_end, conn.next()).await;
        status.lock().unwrap().transport_diagnostics = Some(conn.diagnostics());
        let msg = result.map_err(|_| failure("MISS_AUTH_TIMEOUT", "摄像头视频认证超时"))??;
        if msg.channel == 0 {
            let mut diagnostic = status.lock().unwrap().auth_diagnostics.clone();
            let result = miss::authenticated(&msg.bytes, &grant.key, &mut diagnostic);
            status.lock().unwrap().auth_diagnostics = diagnostic;
            if result? {
                break;
            }
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
        format!("{{\"videoquality\":{quality},\"enableaudio\":0}}").as_bytes(),
    ))
    .await?;
    status.lock().unwrap().stage = "waiting_media";
    let mut received = Instant::now();
    let mut first = None;
    let mut codec = None;
    let mut primed = false;
    let mut ready = false;
    let mut headers = zeroize::Zeroizing::new(Vec::new());
    let mut listening = false;
    let mut speaker_ready_at = None;
    let mut pending_move: Option<PendingMove> = None;
    loop {
        let deadline = tokio::time::Instant::from_std(received + Duration::from_secs(20));
        let message = tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {
                return Err(failure("MISS_MEDIA_TIMEOUT", "摄像头视频数据中断"));
            }
            Some(request) = media.control_rx.recv() => {
                if request.reply.is_closed() {
                    continue;
                }
                let action = match request.action {
                    ControlAction::Move(direction) => {
                        if pending_move.is_some() {
                            let _ = request.reply.send(Err(failure("CAMERA_PTZ_BUSY", "正在等待上一条云台操作响应")));
                        } else {
                            let body = format!(r#"{{"operation":{}}}"#, direction as u8);
                            let wire = miss::encrypted_command(&grant.key, 0x112, body.as_bytes());
                            match tokio::time::timeout(Duration::from_secs(2), conn.command(&wire)).await {
                                Ok(Ok(())) => pending_move = Some(PendingMove {
                                    deadline: tokio::time::Instant::now() + Duration::from_secs(3),
                                    reply: request.reply,
                                }),
                                Ok(Err(error)) => { let _ = request.reply.send(Err(error)); }
                                Err(_) => {
                                    let _ = request.reply.send(Err(failure("CAMERA_PTZ_SEND_TIMEOUT", "发送云台操作超时")));
                                }
                            }
                        }
                        continue;
                    }
                    ControlAction::Media(action) => action,
                };
                let command = match action {
                    MediaControl::ListenOn => 0x104,
                    MediaControl::ListenOff => 0x105,
                    MediaControl::TalkOn => 0x106,
                    MediaControl::TalkOff => 0x108,
                };
                let wire = miss::encrypted_command(&grant.key, command, &[]);
                let result = tokio::time::timeout(Duration::from_secs(2), conn.command(&wire))
                    .await.unwrap_or_else(|_| Err(failure("CAMERA_CONTROL_TIMEOUT", "摄像头控制超时")));
                if result.is_ok() {
                    match action {
                        MediaControl::ListenOn => listening = true,
                        MediaControl::ListenOff => {
                            listening = false;
                            media.audio.lock().unwrap().clear();
                        }
                        MediaControl::TalkOn => {
                            *speaker_stop = Some(zeroize::Zeroizing::new(miss::encrypted_command(&grant.key, 0x108, &[])));
                            speaker_ready_at = Some(Instant::now() + Duration::from_secs(1));
                        }
                        MediaControl::TalkOff => {
                            *speaker_stop = None;
                            speaker_ready_at = None;
                        }
                    }
                }
                let _ = request.reply.send(result);
                continue;
            }
            Some(samples) = media.speaker_rx.recv() => {
                if speaker_ready_at.is_some_and(|time| time <= Instant::now()) {
                    conn.speaker(miss::speaker_packet(&grant.key, &samples));
                }
                continue;
            }
            _ = async {
                match &pending_move {
                    Some(pending) => tokio::time::sleep_until(pending.deadline).await,
                    None => std::future::pending::<()>().await,
                }
            } => {
                if let Some(pending) = pending_move.take() {
                    let _ = pending.reply.send(Err(failure("CAMERA_PTZ_NO_REPLY", "摄像头未返回云台响应，请先查看画面是否已移动")));
                }
                continue;
            }
            message = conn.next() => message?,
        };
        status.lock().unwrap().transport_diagnostics = Some(conn.diagnostics());
        if message.channel == 0 && pending_move.is_some() {
            if let Some(result) = miss::motor_reply(&grant.key, &message.bytes) {
                if let Some(pending) = pending_move.take() {
                    let _ = pending.reply.send(result);
                }
            }
        }
        if message.channel != 2 {
            continue;
        }
        let Some(video) = miss::video(&grant.key, &message.bytes)? else {
            if listening {
                if let Some(packet) = miss::audio(&grant.key, &message.bytes) {
                    let mut audio = media.audio.lock().unwrap();
                    if audio.len() >= 16 {
                        audio.pop_front();
                    }
                    audio.push_back(packet);
                }
            }
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
            *pipeline = Some(MediaOutput::start(video.codec, quality).await?);
        }
        let output = pipeline.as_mut().unwrap();
        if !headers.is_empty() {
            headers.extend(&*video.bytes);
            output
                .write_frame(
                    &headers,
                    video.timestamp,
                    video.keyframe,
                    message.received_at,
                )
                .await?;
            headers.clear();
        } else {
            output
                .write_frame(
                    &video.bytes,
                    video.timestamp,
                    video.keyframe,
                    message.received_at,
                )
                .await?;
        }
        status.lock().unwrap().timing_diagnostics = Some(output.diagnostics());
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
    async fn window_ownership_keeps_floating_playback_independent_and_scopes_stop() {
        let sessions = CameraStreams::default();
        let main = sessions
            .launch_owned("a", "main", 2, std::future::pending())
            .await
            .unwrap();
        let viewer = sessions
            .launch_owned("b", "camera-test", 1, std::future::pending())
            .await
            .unwrap();
        assert!(
            sessions
                .status_owned(&viewer.session_id, "main")
                .await
                .is_err()
        );
        sessions.stop_owned(&viewer.session_id, "main").await;
        assert!(
            sessions
                .status_owned(&viewer.session_id, "camera-test")
                .await
                .is_ok()
        );
        sessions.stop_owner("main").await;
        assert!(
            sessions
                .status_owned(&main.session_id, "main")
                .await
                .is_err()
        );
        assert_eq!(
            sessions
                .status_owned(&viewer.session_id, "camera-test")
                .await
                .unwrap()
                .requested_quality,
            1
        );
        sessions.stop_owner("camera-test").await;
        assert!(
            sessions
                .status_owned(&viewer.session_id, "camera-test")
                .await
                .is_err()
        );
        assert!(
            sessions
                .launch_owned("c", "main", 255, std::future::pending())
                .await
                .is_err()
        );
    }
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
