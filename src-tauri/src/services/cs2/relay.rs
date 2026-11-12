use super::{channel::Receiver, failure, malformed, protocol::*};
use crate::errors::AppError;
use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::UdpSocket,
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{Instant, timeout_at},
};
use zeroize::Zeroizing;

pub type Stage = Arc<dyn Fn(&'static str) + Send + Sync>;
fn io_error() -> AppError {
    failure("CS2_NETWORK", "无法建立摄像头 UDP 连接，请检查网络或防火墙")
}
async fn send(
    socket: &UdpSocket,
    peer: SocketAddrV4,
    kind: u8,
    payload: &[u8],
) -> Result<(), AppError> {
    socket
        .send_to(&frame(kind, payload), peer)
        .await
        .map_err(|_| io_error())?;
    Ok(())
}
async fn receive(
    socket: &UdpSocket,
    until: Instant,
) -> Result<Option<(SocketAddrV4, u8, Vec<u8>)>, AppError> {
    let mut buffer = [0; 4096];
    loop {
        let (n, from) = match timeout_at(until, socket.recv_from(&mut buffer)).await {
            Err(_) => return Ok(None),
            Ok(Err(_)) => return Err(io_error()),
            Ok(Ok(v)) => v,
        };
        if let (SocketAddr::V4(from), Some((kind, body))) = (from, parse(&buffer[..n])) {
            return Ok(Some((from, kind, body.to_vec())));
        }
    }
}
fn window(end: Instant, millis: u64) -> Instant {
    end.min(Instant::now() + Duration::from_millis(millis))
}
fn registration(token: &[u8], uid: &[u8; 20]) -> Zeroizing<Vec<u8>> {
    let mut b = Zeroizing::new(Vec::with_capacity(28));
    b.extend(token);
    b.extend(uid);
    b.extend([0; 4]);
    b
}

pub async fn dial(
    uid: &[u8; 20],
    servers: &[Ipv4Addr],
    stage: Stage,
) -> Result<Connection, AppError> {
    let targets = servers
        .iter()
        .map(|ip| SocketAddrV4::new(*ip, 32100))
        .collect::<Vec<_>>();
    let (socket, relay) = handshake(uid, &targets, stage.clone(), false).await?;
    Connection::open(socket, relay, stage).await
}

async fn handshake(
    uid: &[u8; 20],
    servers: &[SocketAddrV4],
    stage: Stage,
    test_network: bool,
) -> Result<(UdpSocket, SocketAddrV4), AppError> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .await
        .map_err(|_| io_error())?;
    let end = Instant::now() + Duration::from_secs(85);
    let allowed =
        |ip: Ipv4Addr| public_ipv4(ip) || (cfg!(test) && test_network && ip.is_loopback());
    if servers.is_empty() || servers.iter().any(|s| !allowed(*s.ip())) {
        return Err(malformed());
    }
    let mut last = failure("CS2_RENDEZVOUS_TIMEOUT", "连接摄像头会合服务超时");
    for _round in 0..4 {
        if Instant::now() >= end {
            break;
        }
        stage("rendezvous");
        for peer in servers {
            send(&socket, *peer, LIST, uid).await?;
        }
        let mut candidates: BTreeMap<SocketAddrV4, Zeroizing<Vec<[u8; 4]>>> = BTreeMap::new();
        let discovery = window(end, if test_network { 50 } else { 2200 });
        while let Some((from, kind, body)) = receive(&socket, discovery).await? {
            if !servers.contains(&from) || kind != CANDIDATE || body.len() != 20 {
                continue;
            }
            let relay = match address(&body) {
                Ok(a) if allowed(*a.ip()) => a,
                _ => continue,
            };
            let token: [u8; 4] = body[16..20].try_into().unwrap();
            if candidates.len() >= 16 && !candidates.contains_key(&relay) {
                continue;
            }
            let tokens = candidates
                .entry(relay)
                .or_insert_with(|| Zeroizing::new(Vec::new()));
            if tokens.len() < 16 && !tokens.contains(&token) {
                tokens.push(token);
            }
        }
        if candidates.is_empty() {
            continue;
        }
        stage("relay_candidate");
        let mut chosen = None;
        for _ in 0..4 {
            for (peer, tokens) in &candidates {
                for token in tokens.iter() {
                    for _ in 0..4 {
                        send(&socket, *peer, REGISTER, &registration(token, uid)).await?;
                    }
                }
                send(&socket, *peer, HELLO, &[]).await?;
            }
            let slice = window(end, 1500);
            while let Some((from, kind, _)) = receive(&socket, slice).await? {
                if kind == HELLO_ACK && candidates.contains_key(&from) {
                    chosen = Some(from);
                    break;
                }
            }
            if chosen.is_some() {
                break;
            }
        }
        let Some(chosen) = chosen else {
            last = failure("CS2_RELAY_NOT_REGISTERED", "摄像头中继节点未响应注册");
            continue;
        };
        let mut allocation = None;
        for _ in 0..4 {
            send(&socket, chosen, PORT, &[]).await?;
            let slice = window(end, 1500);
            while let Some((from, kind, body)) = receive(&socket, slice).await? {
                if from == chosen && kind == PORT_ACK && body.len() >= 6 {
                    let port = u16::from_be_bytes([body[4], body[5]]);
                    if port != 0 {
                        allocation = Some((
                            SocketAddrV4::new(*chosen.ip(), port),
                            Zeroizing::new(<[u8; 4]>::try_from(&body[..4]).unwrap()),
                        ));
                        break;
                    }
                }
            }
            if allocation.is_some() {
                break;
            }
        }
        let Some((relay, token)) = allocation else {
            last = failure("CS2_SESSION_TIMEOUT", "未取得摄像头中继会话端口");
            continue;
        };
        stage("relay_request");
        let mut request = Zeroizing::new(uid.to_vec());
        request.extend(address_bytes(relay));
        request.extend(&*token);
        for _ in 0..6 {
            for peer in servers {
                send(&socket, *peer, REQUEST, &request).await?;
            }
            // A new relay port needs its own outbound NAT mapping.
            for _ in 0..24 {
                send(&socket, relay, REGISTER, &registration(&*token, uid)).await?;
            }
            let slice = window(end, 2000);
            while let Some((from, kind, _)) = receive(&socket, slice).await? {
                if from == relay && kind == READY {
                    stage("relay_ready");
                    socket.connect(relay).await.map_err(|_| io_error())?;
                    return Ok((socket, relay));
                }
            }
            if Instant::now() >= end {
                break;
            }
        }
        last = failure("CS2_RELAY_NOT_READY", "中继已请求，摄像头尚未加入远程会话");
    }
    Err(last)
}

pub struct Message {
    pub channel: u8,
    pub bytes: Zeroizing<Vec<u8>>,
}
/// Counts only. Never include packet bytes, addresses, UID or relay tokens.
#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportDiagnostics {
    pub link_requests: u32,
    pub link_non_requests: u32,
    pub command_messages: u32,
    pub media_messages: u32,
    pub stream_error_channel: Option<u8>,
}
pub struct Connection {
    outgoing: mpsc::Sender<Zeroizing<Vec<u8>>>,
    incoming: mpsc::Receiver<Message>,
    task: Option<JoinHandle<Result<(), AppError>>>,
    diagnostics: Arc<std::sync::Mutex<TransportDiagnostics>>,
}
impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}
impl Connection {
    async fn open(socket: UdpSocket, _relay: SocketAddrV4, stage: Stage) -> Result<Self, AppError> {
        let SocketAddr::V4(local) = socket.local_addr().map_err(|_| io_error())? else {
            return Err(io_error());
        };
        let (outgoing, rx) = mpsc::channel(16);
        let (tx, incoming) = mpsc::channel(64);
        let (linked, wait_link) = oneshot::channel();
        stage("session_linking");
        let diagnostics = Arc::new(std::sync::Mutex::new(TransportDiagnostics::default()));
        let task = tokio::spawn(transport(
            socket,
            local,
            rx,
            tx,
            linked,
            diagnostics.clone(),
        ));
        let mut conn = Self {
            outgoing,
            incoming,
            task: Some(task),
            diagnostics,
        };
        match tokio::time::timeout(Duration::from_secs(12), wait_link).await {
            Ok(Ok(())) => {
                stage("session_established");
                Ok(conn)
            }
            _ => {
                if conn.task.as_ref().is_some_and(|t| t.is_finished()) {
                    if let Some(t) = conn.task.take() {
                        if let Ok(Err(e)) = t.await {
                            return Err(e);
                        }
                    }
                }
                Err(failure(
                    "CS2_LINK_TIMEOUT",
                    "中继就绪，但摄像头会话连接未完成",
                ))
            }
        }
    }
    /// Message includes the BE command identifier, but not the DRW length prefix.
    pub async fn command(&self, message: &[u8]) -> Result<(), AppError> {
        if message.len() > 32 * 1024 {
            return Err(malformed());
        }
        self.outgoing
            .send(Zeroizing::new(message.to_vec()))
            .await
            .map_err(|_| failure("CS2_CHANNEL_CLOSED", "摄像头连接已关闭"))
    }
    pub async fn next(&mut self) -> Result<Message, AppError> {
        if let Some(msg) = self.incoming.recv().await {
            return Ok(msg);
        }
        if let Some(t) = self.task.take() {
            if let Ok(Err(e)) = t.await {
                return Err(e);
            }
        }
        Err(failure("CS2_CHANNEL_CLOSED", "摄像头远程连接已中断"))
    }
    pub fn diagnostics(&self) -> TransportDiagnostics {
        self.diagnostics.lock().unwrap().clone()
    }
    pub async fn close(&mut self) {
        // Closing the command channel lets the actor send the protocol CLOSE.
        let (replacement, _) = mpsc::channel(1);
        self.outgoing = replacement;
        if let Some(mut task) = self.task.take() {
            if tokio::time::timeout(Duration::from_millis(500), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
    }
}
struct Pending {
    bytes: Zeroizing<Vec<u8>>,
    sent: Instant,
    attempts: u8,
}
async fn transport(
    socket: UdpSocket,
    local: SocketAddrV4,
    mut commands: mpsc::Receiver<Zeroizing<Vec<u8>>>,
    messages: mpsc::Sender<Message>,
    linked: oneshot::Sender<()>,
    diagnostics: Arc<std::sync::Mutex<TransportDiagnostics>>,
) -> Result<(), AppError> {
    let mut linked = Some(linked);
    let mut sequence = 0u16;
    let mut outstanding: BTreeMap<u16, Pending> = BTreeMap::new();
    let mut command_rx = Receiver::default();
    let mut media_rx = Receiver::default();
    let mut buffer = [0; 4096];
    let mut last = Instant::now();
    let mut keep = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            message=commands.recv(), if outstanding.len()<=31 => {
                let Some(message)=message else { let _=socket.send(&frame(CLOSE,&[])).await;return Ok(()); };
                let mut stream=Zeroizing::new((message.len() as u32).to_be_bytes().to_vec());stream.extend(&*message);
                for bytes in stream.chunks(1024) {
                    let mut body=Zeroizing::new(vec![0xd1,0]);body.extend(sequence.to_be_bytes());body.extend(bytes);
                    let packet=Zeroizing::new(frame(DATA,&body));
                    socket.send(&packet).await.map_err(|_|io_error())?;
                    outstanding.insert(sequence,Pending {bytes:packet,sent:Instant::now(),attempts:1});
                    sequence=sequence.wrapping_add(1);
                }
            }
            received=socket.recv(&mut buffer) => {
                let n=received.map_err(|_|io_error())?;
                let Some((kind,body))=parse(&buffer[..n]) else { continue; };
                last=Instant::now();
                match kind {
                    LINK if is_link_request(body) => {
                        {let mut d=diagnostics.lock().unwrap();d.link_requests=d.link_requests.saturating_add(1);}
                        let reply=linking_reply(body,local)?;
                        socket.send(&frame(LINK,&reply)).await.map_err(|_|io_error())?;
                        if let Some(tx)=linked.take() { let _=tx.send(()); }
                    }
                    // 0x28 also carries replies/notifications. Only requests
                    // require rewriting; a confirmation must not tear down MISS.
                    LINK => {let mut d=diagnostics.lock().unwrap();d.link_non_requests=d.link_non_requests.saturating_add(1);}
                    DATA if body.len()>=4 && body[0]==0xd1 && matches!(body[1],0|2) => {
                        let channel=body[1]; let seq=u16::from_be_bytes([body[2],body[3]]);
                        socket.send(&frame(ACK,&[0xd1,channel,0,1,body[2],body[3]])).await.map_err(|_|io_error())?;
                        let rx=if channel==0 { &mut command_rx } else { &mut media_rx };
                        let batch=rx.push(seq,&body[4..]).map_err(|e|{diagnostics.lock().unwrap().stream_error_channel=Some(channel);e})?;
                        for bytes in batch {
                            {let mut d=diagnostics.lock().unwrap();if channel==0 {d.command_messages=d.command_messages.saturating_add(1);}else {d.media_messages=d.media_messages.saturating_add(1);}}
                            messages.try_send(Message {channel,bytes:Zeroizing::new(bytes)}).map_err(|_|failure("CS2_BACKPRESSURE","视频处理速度不足，请停止后重试"))?;
                        }
                    }
                    ACK if body.len()>=4 && body[..2]==[0xd1,0] => {
                        let count=u16::from_be_bytes([body[2],body[3]]) as usize;
                        if count<=64 && body.len()==4+count*2 {
                            for seq in body[4..].as_chunks::<2>().0 { outstanding.remove(&u16::from_be_bytes([seq[0],seq[1]])); }
                        }
                    }
                    ALIVE => { socket.send(&frame(ALIVE_ACK,&[])).await.map_err(|_|io_error())?; }
                    CLOSE => return Err(failure("CS2_CHANNEL_CLOSED","摄像头关闭了远程连接")),
                    _ => {}
                }
            }
            _=tick.tick() => {
                if last.elapsed()>Duration::from_secs(20) { return Err(failure("CS2_CHANNEL_TIMEOUT","摄像头远程连接超时")); }
                if keep.elapsed()>Duration::from_secs(4) {
                    socket.send(&frame(ALIVE,&[])).await.map_err(|_|io_error())?;keep=Instant::now();
                }
                for packet in outstanding.values_mut() {
                    if packet.sent.elapsed()<Duration::from_millis(600) { continue; }
                    if packet.attempts>=8 { return Err(failure("CS2_COMMAND_TIMEOUT","摄像头未确认控制数据")); }
                    socket.send(&packet.bytes).await.map_err(|_|io_error())?;
                    packet.sent=Instant::now();packet.attempts+=1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn simulated_remote_relay_requires_registration_link_and_retransmits_lost_auth() {
        let central = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let node = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let SocketAddr::V4(server) = central.local_addr().unwrap() else {
            panic!()
        };
        let SocketAddr::V4(relay) = node.local_addr().unwrap() else {
            panic!()
        };
        let peer = peer_id("TEST-42-CHECK").unwrap();
        let expected = peer;
        let mock = tokio::spawn(async move {
            let mut a = [0; 4096];
            let mut b = [0; 4096];
            let mut registered = false;
            let mut dropped = false;
            let mut linked = false;
            let mut confirmations_sent = false;
            loop {
                tokio::select! {
                    result=central.recv_from(&mut a)=> {
                        let(n,client)=result.unwrap();let(kind,body)=parse(&a[..n]).unwrap();
                        match kind {
                            LIST=>{assert_eq!(body,&expected);let mut candidate=address_bytes(relay).to_vec();candidate.extend([1,2,3,4]);central.send_to(&frame(CANDIDATE,&candidate),client).await.unwrap();}
                            REQUEST=>{assert_eq!(&body[..20],&expected);assert_eq!(&body[36..40],&[5,6,7,8]);node.send_to(&frame(READY,&[]),client).await.unwrap();let mut req=[0;88];req[..2].copy_from_slice(&[0xa1,1]);node.send_to(&frame(LINK,&req),client).await.unwrap();}
                            _=>{}
                        }
                    }
                    result=node.recv_from(&mut b)=> {
                        let(n,client)=result.unwrap();let(kind,body)=parse(&b[..n]).unwrap();
                        match kind {
                            REGISTER=>{assert_eq!(body.len(),28);registered=true;}
                            HELLO=>{assert!(registered);node.send_to(&frame(HELLO_ACK,&[]),client).await.unwrap();}
                            PORT=>{let mut payload=vec![5,6,7,8];payload.extend(relay.port().to_be_bytes());node.send_to(&frame(PORT_ACK,&payload),client).await.unwrap();}
                            LINK=>{
                                assert_eq!(body[1],2);linked=true;
                                if !confirmations_sent {
                                    confirmations_sent=true;
                                    node.send_to(&frame(LINK,body),client).await.unwrap();
                                    // Duplicate request and reply must remain idempotent.
                                    let mut repeated=body.to_vec();repeated[1]=1;
                                    node.send_to(&frame(LINK,&repeated),client).await.unwrap();
                                }
                            }
                            DATA=>{
                                assert!(linked);
                                if !dropped {dropped=true;continue;}
                                node.send_to(&frame(ACK,&[0xd1,0,0,1,body[2],body[3]]),client).await.unwrap();
                                let mut response=0x101u32.to_le_bytes().to_vec();response.extend(b"{\"result\":\"success\",\"channel\":\"private-channel\"}\0\0");
                                let mut msg=vec![0xd1,0,0,0];msg.extend((response.len() as u32).to_be_bytes());msg.extend(&response);
                                node.send_to(&frame(DATA,&msg),client).await.unwrap();
                            }
                            CLOSE=>return dropped && linked,
                            _=>{}
                        }
                    }
                }
            }
        });
        let stages = Arc::new(std::sync::Mutex::new(Vec::new()));
        let capture = stages.clone();
        let notify: Stage = Arc::new(move |s| capture.lock().unwrap().push(s));
        let (socket, relay) = handshake(&peer, &[server], notify.clone(), true)
            .await
            .unwrap();
        let mut connection = Connection::open(socket, relay, notify).await.unwrap();
        connection
            .command(&crate::services::miss::auth_message("fixture", "fixture"))
            .await
            .unwrap();
        let msg = tokio::time::timeout(Duration::from_secs(3), connection.next())
            .await
            .unwrap()
            .unwrap();
        let mut auth = crate::services::miss::AuthDiagnostics::default();
        assert!(crate::services::miss::authenticated(&msg.bytes, &[0; 32], &mut auth).unwrap());
        assert_eq!(auth.command_byte_order, Some("le"));
        assert!(connection.diagnostics().link_non_requests >= 1);
        assert!(connection.diagnostics().link_requests >= 2);
        connection.close().await;
        assert!(
            tokio::time::timeout(Duration::from_secs(1), mock)
                .await
                .unwrap()
                .unwrap()
        );
        assert!(stages.lock().unwrap().contains(&"session_established"));
    }
}
