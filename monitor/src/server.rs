use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use std::{collections::HashSet, mem};

use actix::clock::{Instant, Interval, interval};
use actix_cloud::{
    chrono::{DateTime, Utc},
    tokio::{
        io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        select, spawn,
        sync::{
            broadcast::{Receiver, Sender, channel},
            mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
        },
        time::{sleep, timeout},
    },
    tracing::{Instrument, Span, debug, error, field, info, info_span, warn},
};
use aes_gcm::aead::{Aead, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, KeyInit, Nonce};
use derivative::Derivative;
use ecies::SecretKey;
use miniz_oxide::deflate::compress_to_vec;
use parking_lot::RwLock;
use skynet_api::service::{self, Service};
use skynet_api::{
    HyUuid, Result, anyhow::anyhow, bail, ffi_rpc::registry::Registry, request::Condition,
    sea_orm::TransactionTrait,
};
use skynet_api_monitor::{
    AgentStatus, CommandRspMessage, FileRspMessage, FrontendMessage, HandshakeReqMessage,
    HandshakeRspMessage, HandshakeStatus, ID, InfoMessage, Message, StatusReqMessage,
    StatusRspMessage, UpdateMessage, frontend_message, message::Data, prost::Message as _,
    viewer::passive_agents::PassiveAgentViewer,
};

use crate::{PLUGIN_INSTANCE, WEBPUSH_ALERT};

const MAX_MESSAGE_SIZE: u32 = 1024 * 1024 * 128;
const AES256_KEY_SIZE: usize = 32;
const SECRET_KEY_SIZE: usize = 32;
const MAGIC_NUMBER: &[u8] = b"SKNT";

#[derive(Derivative)]
#[derivative(Default(new = "true"))]
struct FrameLen {
    data: [u8; 4],
    consumed: usize,
}

impl FrameLen {
    async fn read<R>(&mut self, io: &mut R) -> Result<u32>
    where
        R: AsyncRead + Unpin,
    {
        while self.consumed < 4 {
            let cnt = match io.read(&mut self.data[self.consumed..]).await {
                Ok(x) => x,
                Err(e) => {
                    self.consumed = 0;
                    return Err(e.into());
                }
            };
            if cnt == 0 {
                self.consumed = 0;
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
            }
            self.consumed += cnt;
        }
        Ok(u32::from_be_bytes(self.data))
    }

    fn reset(&mut self) {
        self.consumed = 0;
    }
}

#[derive(Derivative)]
#[derivative(Default(new = "true"))]
struct FrameData {
    data: Vec<u8>,
    len: usize,
    consumed: usize,
}

impl FrameData {
    fn resize(&mut self, len: u32) {
        let len: usize = len.try_into().unwrap();
        self.data.resize(len, 0);
        self.len = len;
    }

    async fn read<R>(&mut self, io: &mut R) -> Result<()>
    where
        R: AsyncRead + Unpin,
    {
        while self.consumed < self.len {
            let cnt = match io.read(&mut self.data[self.consumed..]).await {
                Ok(x) => x,
                Err(e) => {
                    self.consumed = 0;
                    return Err(e.into());
                }
            };
            if cnt == 0 {
                self.consumed = 0;
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
            }
            self.consumed += cnt;
        }
        Ok(())
    }

    fn reset(&mut self) -> Vec<u8> {
        self.consumed = 0;
        mem::take(&mut self.data)
    }
}

struct Frame {
    stream: TcpStream,
    cipher: Option<Aes256Gcm>,
    sk: [u8; SECRET_KEY_SIZE],
    data: FrameData,
    len: FrameLen,
}

impl Frame {
    fn new(stream: TcpStream, sk: SecretKey) -> Self {
        Self {
            stream,
            cipher: None,
            sk: sk.serialize(),
            data: FrameData::new(),
            len: FrameLen::new(),
        }
    }

    async fn close(&mut self) {
        let _ = self.stream.shutdown().await;
    }

    async fn send(&mut self, buf: &[u8]) -> Result<()> {
        let len = buf.len().try_into()?;
        if len > MAX_MESSAGE_SIZE {
            return Err(io::Error::from(io::ErrorKind::InvalidData).into());
        }
        self.stream.write_u32(len).await?;
        self.stream.write_all(buf).await?;
        self.stream.flush().await?;
        Ok(())
    }

    async fn send_msg(&mut self, msg: &Message) -> Result<()> {
        let mut buf = MAGIC_NUMBER.to_vec();
        buf.extend(msg.encode_to_vec());
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let enc = self
            .cipher
            .as_ref()
            .unwrap()
            .encrypt(&nonce, buf.as_slice())
            .map_err(|e| anyhow!(e))?;
        let mut buf = nonce.to_vec();
        buf.extend(enc);
        self.send(&buf).await
    }

    pub async fn read(&mut self, limit: u32) -> Result<Vec<u8>> {
        let len = self.len.read(&mut self.stream).await?;
        if len > limit {
            self.len.reset();
            return Err(io::Error::from(io::ErrorKind::InvalidData).into());
        }
        self.data.resize(len);
        let r = self.data.read(&mut self.stream).await;
        self.len.reset();
        r?;
        Ok(self.data.reset())
    }

    /// Read message from frame.
    ///
    /// # Cancel safety
    /// This function is cancellation safe.
    async fn read_msg(&mut self) -> Result<Message> {
        let buf = if self.cipher.is_some() {
            self.read(MAX_MESSAGE_SIZE).await?
        } else {
            self.read(256).await?
        };
        if let Some(cipher) = &self.cipher {
            let nonce = Nonce::from_slice(&buf[0..12]);
            let buf = cipher.decrypt(nonce, &buf[12..]).map_err(|e| anyhow!(e))?;
            if !buf.starts_with(MAGIC_NUMBER) {
                bail!("Invalid magic number");
            }
            Message::decode(&buf[MAGIC_NUMBER.len()..]).map_err(Into::into)
        } else {
            // handshake
            let data = ecies::decrypt(&self.sk, &buf).map_err(|e| anyhow!(e))?;
            if data.len() > AES256_KEY_SIZE {
                let (key, uid) = data.split_at(AES256_KEY_SIZE);
                self.cipher = Some(Aes256Gcm::new_from_slice(key)?);
                Ok(Message {
                    seq: 0,
                    data: Some(Data::HandshakeReq(HandshakeReqMessage {
                        uid: String::from_utf8_lossy(uid).to_string(),
                    })),
                })
            } else {
                bail!("Invalid handshake data");
            }
        }
    }

    pub async fn read_msg_timeout(&mut self, sec: u32) -> Result<Message> {
        if sec == 0 {
            self.read_msg().await
        } else {
            match timeout(Duration::from_secs(sec.into()), self.read_msg()).await {
                Ok(x) => x,
                Err(_) => Err(anyhow!("Read message timeout")),
            }
        }
    }
}

struct Handler {
    shutdown_rx: Receiver<()>,
    client_seq: u64,
    server_seq: u64,
    trace_id: HyUuid,
    start_time: DateTime<Utc>,
    client_addr: SocketAddr,
    aid: Option<HyUuid>,
    status_clock: Option<Interval>,
    message: Option<UnboundedReceiver<Data>>,
}

impl Handler {
    fn new(trace_id: HyUuid, client_addr: SocketAddr, shutdown_rx: Receiver<()>) -> Self {
        Self {
            client_seq: 0,
            server_seq: 0,
            shutdown_rx,
            trace_id,
            start_time: Utc::now(),
            client_addr,
            aid: None,
            status_clock: None,
            message: None,
        }
    }

    fn new_server_msg(&mut self, data: Data) -> Message {
        let ret = Message {
            seq: self.server_seq,
            data: Some(data),
        };
        self.server_seq += 1;
        ret
    }

    async fn handshake(&mut self, frame: &mut Frame, msg: Message) -> Result<()> {
        if msg.seq == 0 && self.client_seq == 0 {
            if let Some(Data::HandshakeReq(data)) = msg.data {
                let tx = PLUGIN_INSTANCE.db.get().unwrap().begin().await?;
                self.aid = Some(
                    if let Some(x) = PLUGIN_INSTANCE
                        .login(&tx, &data.uid, &self.client_addr)
                        .await?
                    {
                        x
                    } else {
                        let _ = frame
                            .send_msg(&self.new_server_msg(Data::HandshakeRsp(
                                HandshakeRspMessage {
                                    status: HandshakeStatus::Logined.into(),
                                    trace_id: self.trace_id.to_string(),
                                },
                            )))
                            .await;
                        bail!("Already login");
                    },
                );
                tx.commit().await?;

                self.message = Some(PLUGIN_INSTANCE.bind_message(&self.aid.unwrap()));
                Span::current().record("aid", self.aid.unwrap().to_string());
                self.start_time = Utc::now();
                info!(
                    _time = self.start_time.timestamp_micros(),
                    "Agent connection received"
                );
                return frame
                    .send_msg(
                        &self.new_server_msg(Data::HandshakeRsp(HandshakeRspMessage {
                            status: HandshakeStatus::Success.into(),
                            trace_id: self.trace_id.to_string(),
                        })),
                    )
                    .await;
            }
        }
        bail!("Invalid handshake message")
    }

    fn handle_status(&mut self, _frame: &mut Frame, data: StatusRspMessage) -> Result<()> {
        PLUGIN_INSTANCE.update_status(&self.aid.unwrap(), data);
        Ok(())
    }

    async fn handle_info(&mut self, frame: &mut Frame, data: InfoMessage) -> Result<()> {
        let aid = self.aid.unwrap();
        let sys = data.os.clone().unwrap_or_default();
        let arch = data.arch.clone().unwrap_or_default();
        let version = data.version.clone();

        debug!(%aid, sys, arch, version, "Agent info message received");

        if data.report_rate != 0 {
            self.status_clock = Some(interval(Duration::from_secs(data.report_rate.into())));
        }

        PLUGIN_INSTANCE
            .update_agent(PLUGIN_INSTANCE.db.get().unwrap(), &self.aid.unwrap(), data)
            .await?;
        if let Some(x) = PLUGIN_INSTANCE.agent_api.get() {
            if x.check_version(&Registry::default(), &version).await {
                info!(agent_version = version, "Updating agent");
                let sys = skynet_api_agent::System::parse(&sys);
                let arch = skynet_api_agent::Arch::parse(&arch);
                if sys.is_none() || arch.is_none() {
                    warn!(
                        arch = ?arch,
                        system = ?sys,
                        "Agent not update, platform invalid",
                    );
                }

                if let Some(data) = x
                    .get_binary(&Registry::default(), &sys.unwrap(), &arch.unwrap())
                    .await
                {
                    if let Some(mut x) = PLUGIN_INSTANCE.agent.get_mut(&aid) {
                        x.status = AgentStatus::Updating;
                    }
                    let crc = crc32fast::hash(&data);
                    let data = compress_to_vec(&data, 6);
                    frame
                        .send_msg(
                            &self.new_server_msg(Data::Update(UpdateMessage { data, crc32: crc })),
                        )
                        .await?;
                } else {
                    let file = x
                        .get_binary_name(&Registry::default(), &sys.unwrap(), &arch.unwrap())
                        .await;
                    warn!(
                        file = %file.to_string_lossy(),
                        "Agent not update, file not found",
                    );
                }
            }
        }
        Ok(())
    }

    fn handle_file(&mut self, _frame: &mut Frame, data: FileRspMessage) -> Result<()> {
        PLUGIN_INSTANCE.update_file_response(
            &self.aid.unwrap(),
            &HyUuid::parse(&data.id)?,
            data.code,
            &data.message,
        );
        Ok(())
    }

    fn handle_command(&mut self, _frame: &mut Frame, data: CommandRspMessage) -> Result<()> {
        PLUGIN_INSTANCE.update_command_output(
            &self.aid.unwrap(),
            &HyUuid::parse(&data.id)?,
            data.code,
            data.output,
        );
        Ok(())
    }

    async fn handle_msg(&mut self, frame: &mut Frame, msg: Message) -> Result<()> {
        if msg.seq < self.client_seq {
            debug!(
                seq = self.client_seq,
                msg_seq = msg.seq,
                "Invalid sequence number"
            );
            Ok(())
        } else {
            self.client_seq = msg.seq + 1;
            if let Some(data) = msg.data {
                match data {
                    Data::Info(data) => self.handle_info(frame, data).await,
                    Data::StatusRsp(data) => self.handle_status(frame, data),
                    Data::ShellOutput(mut data) => {
                        let id = HyUuid::parse(&data.token.unwrap_or_default())?;
                        data.token = None;
                        if let Some(id) = PLUGIN_INSTANCE.shell_binding.get(&id) {
                            if let Some(inst) = PLUGIN_INSTANCE.shell.get(&id) {
                                let _ = inst
                                    .send(FrontendMessage {
                                        id: None,
                                        data: Some(frontend_message::Data::ShellOutput(data)),
                                    })
                                    .await;
                            }
                        }
                        Ok(())
                    }
                    Data::ShellError(mut data) => {
                        let id = HyUuid::parse(&data.token.unwrap_or_default())?;
                        data.token = None;
                        if let Some(id) = PLUGIN_INSTANCE.shell_binding.get(&id) {
                            if let Some(inst) = PLUGIN_INSTANCE.shell.get(&id) {
                                let _ = inst
                                    .send(FrontendMessage {
                                        id: None,
                                        data: Some(frontend_message::Data::ShellError(data)),
                                    })
                                    .await;
                            }
                        }
                        Ok(())
                    }
                    Data::FileRsp(data) => self.handle_file(frame, data),
                    Data::CommandRsp(data) => self.handle_command(frame, data),
                    _ => bail!("Invalid message type"),
                }
            } else {
                bail!("Invalid message")
            }
        }
    }

    async fn get_status_tick(c: &mut Option<Interval>) -> Option<Instant> {
        match c {
            Some(t) => Some(t.tick().await),
            None => None,
        }
    }

    async fn get_proxy_message(c: &mut Option<UnboundedReceiver<Data>>) -> Option<Data> {
        match c {
            Some(d) => d.recv().await,
            None => None,
        }
    }

    async fn send_status(&mut self, frame: &mut Frame) {
        let _ = frame
            .send_msg(&self.new_server_msg(Data::StatusReq(StatusReqMessage {
                time: Utc::now().timestamp_millis(),
            })))
            .await;
    }

    async fn process(&mut self, stream: TcpStream, key: SecretKey) {
        let mut frame = Frame::new(stream, key);
        loop {
            select! {
                msg = frame.read_msg_timeout(*PLUGIN_INSTANCE.msg_timeout.read()) => {
                    match msg {
                        Ok(msg) => {
                            if self.aid.is_none() {
                                if let Err(e) = self.handshake(&mut frame, msg).await {
                                    debug!(error = %e, "Error handshake");
                                    frame.close().await;
                                }
                            } else if let Err(e) = self.handle_msg(&mut frame, msg).await {
                                debug!(error = %e, "Error handle message");
                            }
                        }
                        Err(e) => {
                            if self.aid.is_some() {
                                let end_time = Utc::now();
                                let time = (end_time - self.start_time).num_microseconds().unwrap_or(0);
                                info!(_time = end_time.timestamp_micros(), alive_time = time, error = %e, "Connection lost");
                            }
                            break;
                        }
                    }
                }
                Some(_) = Self::get_status_tick(&mut self.status_clock) => {
                    self.send_status(&mut frame).await;
                }
                Some(data) = Self::get_proxy_message(&mut self.message) => {
                    if let Err(e) = frame.send_msg(&self.new_server_msg(data)).await {
                        debug!(error = %e, "Error send message");
                    }
                }
                _ = self.shutdown_rx.recv() =>{
                    if self.aid.is_some() {
                        let end_time = Utc::now();
                        let time = (end_time - self.start_time).num_microseconds().unwrap_or(0);
                        info!(_time = end_time.timestamp_micros(), alive_time = time, "Server shutdown");
                    }
                    break;
                }
            }
        }
        self.status_clock = None;
        if let Some(aid) = self.aid {
            PLUGIN_INSTANCE.logout(&aid);
            self.message = None;
        }
    }
}

struct Listener {
    listener: TcpListener,
    passive_rx: UnboundedReceiver<HyUuid>,
    passive_agent: Arc<RwLock<HashSet<HyUuid>>>,
    shutdown_rx: Receiver<()>,
    alert_clock: Interval,
}

impl Listener {
    async fn new(
        addr: &str,
        passive_rx: UnboundedReceiver<HyUuid>,
        passive_agent: Arc<RwLock<HashSet<HyUuid>>>,
        shutdown_rx: Receiver<()>,
    ) -> Result<Self> {
        let listener = TcpListener::bind(&addr).await?;
        Ok(Self {
            listener,
            passive_rx,
            passive_agent,
            shutdown_rx,
            alert_clock: interval(Duration::from_secs(5)),
        })
    }

    async fn passive(addr: &str, key: SecretKey, rx: Receiver<()>) -> Result<()> {
        info!(plugin = %ID, "Monitor connecting to {}...", addr);
        let stream = TcpStream::connect(addr).await?;
        let addr = stream.peer_addr()?;
        let trace_id = HyUuid::new();
        Handler::new(trace_id, addr, rx)
            .process(stream, key)
            .instrument(info_span!("Agent connection", plugin = %ID, trace_id = %trace_id, ip = addr.to_string(), aid = field::Empty))
            .await;
        Ok(())
    }

    async fn passive_loop(key: SecretKey, rx: Receiver<()>, apid: HyUuid) -> Result<()> {
        loop {
            let m =
                PassiveAgentViewer::find_by_id(PLUGIN_INSTANCE.db.get().unwrap(), &apid).await?;
            if let Some(m) = m {
                if let Err(e) = Self::passive(&m.address, key, rx.resubscribe()).await {
                    info!(plugin = %ID, error = %e, apid = %apid, address = m.address, "Monitor connect error");
                }
                if m.retry_time != 0 {
                    sleep(Duration::from_secs(m.retry_time.try_into()?)).await;
                } else {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }
    }

    async fn run(&mut self, key: SecretKey, service: Service) {
        let mut alerted: HashMap<HyUuid, i64> = HashMap::new();
        loop {
            select! {
                _ = self.alert_clock.tick() => {
                    let now = Utc::now().timestamp_millis();
                    let timeout = *PLUGIN_INSTANCE.alert_timeout.read();
                    if timeout != 0 {
                        for i in &PLUGIN_INSTANCE.agent {
                            if let Some(x) = i.last_rsp {
                                if i.status != AgentStatus::Online &&
                                    alerted.get(&i.id).map(ToOwned::to_owned).unwrap_or_default() != x &&
                                    (now - x) > (timeout * 1000).into() {
                                    alerted.insert(i.id, x);
                                    service.webpush_send(&Registry::default(), &WEBPUSH_ALERT, &service::Message{
                                        title: String::from("Warning"),
                                        body: format!("Agent `{}` is offline for {timeout} seconds", i.name),
                                        url: format!("/plugin/{ID}/view"),
                                    }).await;
                                }
                            }
                        }
                    }
                },
                c = self.listener.accept() => {
                    match c {
                        Ok((stream, addr)) => {
                            let rx = self.shutdown_rx.resubscribe();
                            spawn(async move {
                                let trace_id = HyUuid::new();
                                Handler::new(trace_id, addr, rx)
                                    .process(stream, key)
                                    .instrument(info_span!("Agent connection", plugin = %ID, trace_id = %trace_id, ip = addr.to_string(), aid = field::Empty))
                                    .await;
                            });
                        }
                        Err(e) => debug!("{e}"),
                    }
                },
                Some(apid) = self.passive_rx.recv() => {
                    let rx = self.shutdown_rx.resubscribe();
                    let passive_agent = self.passive_agent.clone();
                    spawn(async move {
                        passive_agent.write().insert(apid);
                        if let Err(e) =Self::passive_loop(key, rx, apid).await{
                            error!(plugin = %ID, error = %e, apid = %apid, "Monitor passive agent error");
                        }
                        passive_agent.write().remove(&apid);
                    });
                }
                _ = self.shutdown_rx.recv() => {
                    return;
                }
            }
        }
    }
}

#[derive(Derivative)]
#[derivative(Default(new = "true"))]
pub struct Server {
    running: RwLock<bool>,
    service: OnceLock<Service>,
    passive_channel: RwLock<Option<UnboundedSender<HyUuid>>>,
    passive_agent: Arc<RwLock<HashSet<HyUuid>>>,
    shutdown_tx: RwLock<Option<Sender<()>>>,
}

impl Server {
    pub fn init(&self, service: Service) {
        let _ = self.service.set(service);
    }

    pub async fn start(&self, addr: &str, key: SecretKey) -> Result<()> {
        let (tx, mut rx) = channel(1);
        let (passive_tx, passive_rx) = unbounded_channel();
        let mut listener =
            Listener::new(addr, passive_rx, self.passive_agent.clone(), tx.subscribe()).await?;
        *self.passive_channel.write() = Some(passive_tx);
        *self.shutdown_tx.write() = Some(tx);
        *self.running.write() = true;

        let passive = PassiveAgentViewer::find(
            PLUGIN_INSTANCE.db.get().unwrap(),
            Condition::new(Condition::all()),
        )
        .await?
        .0;
        for i in passive {
            self.connect(&i.id);
        }

        info!(plugin = %ID, "Monitor server listening on {addr}");
        select! {
            _ = listener.run(key, self.service.get().unwrap().clone()) => {},
            _ = rx.recv() => {},
        }
        *self.running.write() = false;
        *self.shutdown_tx.write() = None;
        *self.passive_channel.write() = None;
        info!(plugin = %ID, "Monitor server stopped");
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        *self.running.read()
    }

    pub fn stop(&self) -> bool {
        self.shutdown_tx
            .read()
            .as_ref()
            .is_some_and(|x| x.send(()).is_ok())
    }

    pub fn connect(&self, apid: &HyUuid) -> bool {
        if !self.connecting().contains(apid) {
            self.passive_channel
                .read()
                .as_ref()
                .is_some_and(|x| x.send(*apid).is_ok())
        } else {
            true
        }
    }

    pub fn connecting(&self) -> Vec<HyUuid> {
        self.passive_agent
            .read()
            .iter()
            .map(ToOwned::to_owned)
            .collect()
    }
}
