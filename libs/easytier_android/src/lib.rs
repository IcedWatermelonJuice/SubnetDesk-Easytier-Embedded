//! 仅供 Android :easytier 进程使用的核心桥接，不加载桌面宿主或启动 CLI。
#![cfg(target_os = "android")]

mod profile;
// 复用经过 RC8 验收的展示关联算法。它只接收数据，不参与 TUN 或来源授权。
#[path = "../../../src/easytier/network_view.rs"]
mod network_view;

use anyhow::{anyhow, bail, Result};
use easytier::{
    common::{config::{ConfigFileControl, ConfigLoader, TomlConfigLoader}, global_ctx::GlobalCtxEvent},
    instance_manager::NetworkInstanceManager,
    launcher::NetworkInstanceRunningInfo,
};
use jni::{objects::{JObject, JString}, sys::{jint, jstring}, JNIEnv};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    net::Ipv4Addr,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{mpsc as reply_channel, Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(8);
const START_TIMEOUT: Duration = Duration::from_secs(40);
const TUN_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_NODES: usize = 1024;
const MAX_STATUS_BYTES: usize = 240 * 1024;

enum Command {
    Start(String, reply_channel::SyncSender<Value>),
    SetTun(OwnedFd, reply_channel::SyncSender<Value>),
    Stop(reply_channel::SyncSender<Value>),
}

struct Bridge {
    sender: mpsc::UnboundedSender<Command>,
    snapshot: Arc<Mutex<Value>>,
}
static BRIDGE: OnceLock<Result<Bridge, String>> = OnceLock::new();

fn stopped() -> Value {
    json!({"ok":true,"state":"stopped","ipv4":"","nodes":[],"routes":[],
        "allowed_ips":[],"tun_routes":[],"mtu":1380,"tun_ready":false,
        "allow_default_route":false,"instance_id":"","queried_at":null,"error":""})
}

fn failed(message: &str) -> Value {
    let mut value = stopped();
    value["ok"] = json!(false);
    value["state"] = json!("failed");
    value["error"] = json!(message);
    value
}

fn bridge() -> Result<&'static Bridge> {
    let value = BRIDGE.get_or_init(|| {
        let (sender, receiver) = mpsc::unbounded_channel();
        let snapshot = Arc::new(Mutex::new(stopped()));
        let shared = snapshot.clone();
        // 这是此独立进程桥接的 runtime 入口；不在 JNI 调用或异步助手中嵌套 runtime。
        std::thread::Builder::new().name("easytier-android".into()).spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()
                    .map_err(|_| anyhow!("无法初始化 EasyTier 运行环境"))?;
                runtime.block_on(Worker::new(shared.clone()).run(receiver));
                Ok::<_, anyhow::Error>(())
            }));
            if !matches!(result, Ok(Ok(()))) {
                if let Ok(mut value) = shared.lock() { *value = failed("EasyTier 工作线程已异常结束"); }
            }
        }).map_err(|_| "无法创建 EasyTier 工作线程".to_string())?;
        Ok(Bridge { sender, snapshot })
    });
    value.as_ref().map_err(|message| anyhow!(message.clone()))
}

struct Worker {
    manager: Arc<NetworkInstanceManager>,
    snapshot: Arc<Mutex<Value>>,
    id: Option<Uuid>,
    events: Option<broadcast::Receiver<GlobalCtxEvent>>,
    tun: Option<OwnedFd>,
    retired_tuns: Vec<OwnedFd>,
    tun_requested: Option<Instant>,
    tun_ready: bool,
    started: Option<Instant>,
    latency_first: bool,
    mtu: u32,
    exit_nodes: Vec<String>,
}

impl Worker {
    fn new(snapshot: Arc<Mutex<Value>>) -> Self {
        Self { manager: Arc::new(NetworkInstanceManager::new()), snapshot,
            id: None, events: None, tun: None, retired_tuns: vec![], tun_requested: None,
            tun_ready: false, started: None, latency_first: false, mtu: 1380, exit_nodes: vec![] }
    }

    fn publish(&self, value: Value) {
        if let Ok(mut snapshot) = self.snapshot.lock() { *snapshot = value; }
    }

    async fn stop(&mut self) -> Result<()> {
        self.tun_ready = false;
        if let Some(id) = self.id {
            let mut snapshot = stopped();
            snapshot["state"] = json!("stopping");
            snapshot["instance_id"] = json!(id.to_string());
            self.publish(snapshot);
            let manager = self.manager.clone();
            // 上游析构会 join 核心线程；在 blocking pool 完成后才关闭复制的 fd。
            tokio::task::spawn_blocking(move || manager.delete_network_instance(vec![id]))
                .await.map_err(|_| anyhow!("EasyTier 停止任务异常"))??;
        }
        self.id = None;
        self.events = None;
        self.tun = None;
        self.retired_tuns.clear();
        self.tun_requested = None;
        self.started = None;
        Ok(())
    }

    async fn start(&mut self, config: &str) -> Result<Value> {
        profile::validate(config)?;
        let cfg = TomlConfigLoader::new_from_str(config).map_err(|_| anyhow!("EasyTier TOML 无效"))?;
        let mut flags = cfg.get_flags();
        if flags.accept_dns { bail!("Android 当前不接管系统 DNS，请关闭 accept_dns 后启动"); }
        if flags.proxy_forward_by_system || flags.no_tun || flags.enable_udp_broadcast_relay {
            bail!("Android 方案不支持系统转发、no_tun 或物理网卡 UDP 广播捕获");
        }
        if !(576..=9000).contains(&flags.mtu) { bail!("TUN MTU 必须在 576 至 9000 之间"); }
        // 网络绑定由 VpnService 所在独立进程处理，不要求 SO_BINDTODEVICE 权限。
        flags.bind_device = false;
        cfg.set_flags(flags.clone());
        let id = Uuid::new_v4();
        cfg.set_id(id);
        self.stop().await?;
        self.latency_first = flags.latency_first;
        self.mtu = flags.mtu;
        self.exit_nodes = cfg.get_exit_nodes().iter().map(ToString::to_string).collect();
        self.manager.run_network_instance(cfg, false, ConfigFileControl::STATIC_CONFIG)
            .map_err(|_| anyhow!("无法启动 EasyTier 实例"))?;
        self.events = self.manager.iter().find(|item| *item.key() == id)
            .and_then(|instance| instance.subscribe_event());
        self.id = Some(id);
        self.started = Some(Instant::now());
        let mut snapshot = stopped();
        snapshot["state"] = json!("starting");
        snapshot["instance_id"] = json!(id.to_string());
        self.publish(snapshot);
        Ok(json!({"ok":true,"instance_id":id.to_string()}))
    }

    fn set_tun(&mut self, fd: OwnedFd) -> Result<Value> {
        let id = self.id.ok_or_else(|| anyhow!("EasyTier 尚未启动"))?;
        if self.tun_requested.is_some() { bail!("正在切换 TUN，请等待当前切换完成"); }
        self.manager.set_tun_fd(&id, fd.as_raw_fd()).map_err(|_| anyhow!("无法交付 TUN 文件描述符"))?;
        if let Some(old) = self.tun.replace(fd) { self.retired_tuns.push(old); }
        self.tun_requested = Some(Instant::now());
        self.tun_ready = false;
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot["state"] = json!("starting");
            snapshot["tun_ready"] = json!(false);
            snapshot["allowed_ips"] = json!([]);
        }
        Ok(json!({"ok":true,"instance_id":id.to_string()}))
    }

    async fn poll(&mut self) -> Result<()> {
        let Some(id) = self.id else { return Ok(()); };
        if let Some(receiver) = self.events.as_mut() {
            loop {
                match receiver.try_recv() {
                    Ok(GlobalCtxEvent::TunDeviceReady(name)) => {
                        if self.tun.as_ref().is_some_and(|fd| name == format!("tunfd_{}", fd.as_raw_fd())) {
                            self.tun_ready = true;
                            self.tun_requested = None;
                            self.retired_tuns.clear();
                        }
                    }
                    Ok(GlobalCtxEvent::TunDeviceError(_)) => bail!("EasyTier 无法使用 Android TUN"),
                    Ok(_) => {},
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Lagged(_)) => bail!("EasyTier 状态事件丢失，请重新启动组网"),
                    Err(broadcast::error::TryRecvError::Closed) => bail!("EasyTier 运行已结束"),
                }
            }
        }
        if self.tun_requested.is_some_and(|time| time.elapsed() > TUN_TIMEOUT) { bail!("Android TUN 建立超时"); }
        let info = tokio::time::timeout(Duration::from_secs(2), self.manager.get_network_info(&id)).await
            .map_err(|_| anyhow!("EasyTier 状态查询超时"))?;
        if let Some(info) = info {
            if info.error_msg.is_some() || !info.running { bail!("EasyTier 引擎已退出，请检查方案和网络"); }
            let value = snapshot(&info, id, self.latency_first, self.tun_ready, self.mtu, &self.exit_nodes)?;
            let ready = value["state"] == "running";
            self.publish(value);
            if ready { self.started = None; }
        } else if self.started.is_none() {
            bail!("EasyTier 运行状态已不可用");
        }
        if self.started.is_some_and(|time| time.elapsed() > START_TIMEOUT) { bail!("EasyTier 未能在规定时间内取得虚拟地址并建立 TUN"); }
        Ok(())
    }

    async fn run(mut self, mut receiver: mpsc::UnboundedReceiver<Command>) {
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                command = receiver.recv() => match command {
                    Some(Command::Start(config, reply)) => {
                        let result = self.start(&config).await;
                        let value = match result {
                            Ok(value) => value,
                            Err(error) => {
                                let cleanup = self.stop().await;
                                let message = if cleanup.is_ok() { error.to_string() } else { "EasyTier 启动失败且未能完成停止".into() };
                                failed(&message)
                            }
                        };
                        if value["ok"] == false { self.publish(value.clone()); }
                        let _ = reply.send(value);
                    }
                    Some(Command::SetTun(fd, reply)) => {
                        let value = self.set_tun(fd).unwrap_or_else(|error| failed(&error.to_string()));
                        let _ = reply.send(value);
                    }
                    Some(Command::Stop(reply)) => {
                        let result = self.stop().await;
                        let value = if result.is_ok() { stopped() } else { failed("EasyTier 未能完成停止") };
                        self.publish(value.clone());
                        let _ = reply.send(value);
                    }
                    None => { let _ = self.stop().await; break; }
                },
                _ = tick.tick(), if self.id.is_some() => if let Err(error) = self.poll().await {
                    let cleanup = self.stop().await;
                    let message = if cleanup.is_ok() { error.to_string() } else { "EasyTier 故障且未能完成停止".into() };
                    self.publish(failed(&message));
                },
            }
        }
    }
}

fn normalized_route(value: &str) -> Option<String> {
    let (address, prefix) = value.split_once('/').unwrap_or((value, "32"));
    let address: Ipv4Addr = address.parse().ok()?;
    let prefix: u8 = prefix.parse().ok().filter(|bits| *bits <= 32)?;
    if address.is_loopback() || address.is_multicast() { return None; }
    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
    Some(format!("{}/{}", Ipv4Addr::from(u32::from(address) & mask), prefix))
}

fn snapshot(info: &NetworkInstanceRunningInfo, id: Uuid, latency_first: bool, tun_ready: bool, mtu: u32, exit_nodes: &[String]) -> Result<Value> {
    let mut peers = Vec::new();
    let mut routes = Vec::new();
    let mut legacy_routes = Vec::new();
    let mut tun_routes = BTreeSet::new();
    let mut allowed_ips = BTreeSet::new();
    let my_info = info.my_node_info.as_ref();
    let local_id = my_info.map(|node| node.peer_id);
    let ipv4 = my_info.and_then(|node| node.virtual_ipv4.as_ref()).map(ToString::to_string)
        .filter(|cidr| cidr.split('/').next().and_then(|value| value.parse::<Ipv4Addr>().ok())
            .is_some_and(|ip| !ip.is_unspecified() && !ip.is_loopback() && !ip.is_multicast()))
        .unwrap_or_default();
    if let Some(route) = normalized_route(&ipv4).filter(|route| route != "0.0.0.0/0") { tun_routes.insert(route); }
    let allow_default_route = info.routes.iter().any(|route| {
        let address = route.ipv4_addr.as_ref().and_then(|ip| ip.address.as_ref()).map(ToString::to_string);
        address.as_ref().is_some_and(|ip| exit_nodes.contains(ip)) && route.cost > 0
            && route.proxy_cidrs.iter().any(|cidr| normalized_route(cidr).as_deref() == Some("0.0.0.0/0"))
    });
    for pair in info.peer_route_pairs.iter().take(MAX_NODES) {
        let Some(route) = pair.route.as_ref() else { continue; };
        if Some(route.peer_id) == local_id { continue; }
        let ip = route.ipv4_addr.as_ref().and_then(|ip| ip.address.as_ref()).map(ToString::to_string).unwrap_or_default();
        if let Ok(address) = ip.parse::<Ipv4Addr>() {
            if !address.is_unspecified() && !address.is_loopback() { allowed_ips.insert(address.to_string()); }
        }
        if let Some(route) = normalized_route(&ip) { tun_routes.insert(route); }
        for cidr in &route.proxy_cidrs {
            if let Some(route) = normalized_route(cidr) {
                if route != "0.0.0.0/0" || allow_default_route { tun_routes.insert(route); }
            }
        }
        let cost = if route.cost == 1 { "p2p".to_owned() } else { format!("relay({})", route.cost) };
        let nat = route.stun_info.as_ref().and_then(|stun| easytier::proto::common::NatType::try_from(stun.udp_nat_type).ok())
            .map(|nat| format!("{nat:?}")).unwrap_or_default();
        let row = json!({"id":route.peer_id.to_string(),"hostname":route.hostname,"ipv4":ip,
            "cidr":route.ipv4_addr.as_ref().map(ToString::to_string).unwrap_or_default(),"cost":cost,
            "lat_ms":pair.get_latency_ms().map(|v| v.to_string()).unwrap_or_default(),
            "tunnel_proto":pair.get_conn_protos().unwrap_or_default().join(","),
            "loss_rate":pair.get_loss_rate().map(|v| format!("{:.1}%",v*100.0)).unwrap_or_default(),
            "rx_bytes":pair.get_rx_bytes().map(|v| format!("{v} B")).unwrap_or_default(),
            "tx_bytes":pair.get_tx_bytes().map(|v| format!("{v} B")).unwrap_or_default(),
            "nat_type":nat,"version":route.version});
        peers.push(serde_json::from_value::<network_view::PeerRow>(row)?);
    }
    let hop = |peer_id| info.routes.iter().find(|route| route.peer_id == peer_id);
    for route in info.routes.iter().filter(|route| Some(route.peer_id) != local_id).take(MAX_NODES) {
        let addr = route.ipv4_addr.as_ref().map(ToString::to_string).unwrap_or_default();
        let basic = hop(route.next_hop_peer_id);
        let latency = route.next_hop_peer_id_latency_first.and_then(hop);
        let hop_ip = |item: Option<&easytier::proto::api::instance::Route>| item.and_then(|r| r.ipv4_addr.as_ref()).map(ToString::to_string).unwrap_or_default();
        let row = json!({"ipv4":addr,"hostname":route.hostname,"proxy_cidrs":route.proxy_cidrs.join(","),
            "next_hop_ipv4":if route.cost==1 {"DIRECT".into()} else {hop_ip(basic)},
            "next_hop_hostname":if route.cost==1 {"DIRECT"} else {basic.map(|r|r.hostname.as_str()).unwrap_or_default()},
            "path_len":route.cost,"path_latency":route.path_latency,
            "next_hop_ipv4_lat_first":if route.cost_latency_first==Some(1) {"DIRECT".into()} else {hop_ip(latency)},
            "next_hop_hostname_lat_first":if route.cost_latency_first==Some(1) {"DIRECT"} else {latency.map(|r|r.hostname.as_str()).unwrap_or_default()},
            "path_len_lat_first":route.cost_latency_first.unwrap_or_default(),
            "path_latency_lat_first":route.path_latency_latency_first.unwrap_or_default()});
        let use_latency = latency_first && route.cost_latency_first.is_some_and(|cost|cost>0);
        legacy_routes.push(json!({"destination":addr,"next_hop":if use_latency {row["next_hop_ipv4_lat_first"].clone()} else {row["next_hop_ipv4"].clone()},
            "hops":if use_latency {route.cost_latency_first.unwrap_or_default()} else {route.cost},
            "latency":if use_latency {route.path_latency_latency_first.unwrap_or_default()} else {route.path_latency}}));
        routes.push(serde_json::from_value::<network_view::RouteRow>(row)?);
    }
    let state = if tun_ready && !ipv4.is_empty() { "running" } else { "starting" };
    let queried_at = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| anyhow!("系统时间无效"))?.as_millis() as u64;
    let value = json!({"ok":true,"state":state,"instance_id":id.to_string(),"ipv4":ipv4,
        "nodes":network_view::build_nodes(&peers,&routes,latency_first,MAX_NODES),"routes":legacy_routes,
        "allowed_ips":if state=="running" {allowed_ips.into_iter().collect::<Vec<_>>()} else {vec![]},
        "tun_routes":tun_routes.into_iter().collect::<Vec<_>>(),"allow_default_route":allow_default_route,
        "mtu":mtu,"tun_ready":tun_ready,"queried_at":queried_at,"error":""});
    if serde_json::to_vec(&value)?.len() > MAX_STATUS_BYTES {
        bail!("组网状态超出 Android 通信容量，请减少节点或代理子网后重试");
    }
    Ok(value)
}

fn dispatch(make: impl FnOnce(reply_channel::SyncSender<Value>) -> Command) -> Result<Value> {
    let bridge = bridge()?;
    let (send, receive) = reply_channel::sync_channel(1);
    bridge.sender.send(make(send)).map_err(|_| anyhow!("EasyTier 工作线程不可用"))?;
    receive.recv_timeout(COMMAND_TIMEOUT).map_err(|_| anyhow!("EasyTier 操作超时，尚未确认完成"))
}

fn input(env: &mut JNIEnv<'_>, value: JString<'_>, limit: usize) -> Result<String> {
    let value: String = env.get_string(&value).map_err(|_| anyhow!("无法读取调用参数"))?.into();
    if value.len() > limit { bail!("调用参数过大"); }
    Ok(value)
}

fn output(env: &mut JNIEnv<'_>, action: impl FnOnce(&mut JNIEnv<'_>) -> Result<Value>) -> jstring {
    let value = match catch_unwind(AssertUnwindSafe(|| action(env))) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => failed(&error.to_string()),
        Err(_) => failed("EasyTier 原生桥接异常"),
    };
    match env.new_string(value.to_string()) {
        Ok(string) => string.into_raw(),
        Err(_) => std::ptr::null_mut(), // JVM 内存不足时保留待处理异常，不返回伪成功。
    }
}

#[no_mangle]
pub extern "system" fn Java_com_carriez_flutter_1hbb_EasyTierNative_start(mut env: JNIEnv, _: JObject, config: JString) -> jstring {
    output(&mut env, |env| { let config = input(env, config, profile::MAX_TOML)?; dispatch(|reply| Command::Start(config, reply)) })
}

#[no_mangle]
pub extern "system" fn Java_com_carriez_flutter_1hbb_EasyTierNative_status(mut env: JNIEnv, _: JObject) -> jstring {
    output(&mut env, |_| { Ok(bridge()?.snapshot.lock().map_err(|_| anyhow!("EasyTier 状态不可用"))?.clone()) })
}

#[no_mangle]
pub extern "system" fn Java_com_carriez_flutter_1hbb_EasyTierNative_setTunFd(mut env: JNIEnv, _: JObject, fd: jint) -> jstring {
    output(&mut env, |_| {
        if fd < 0 { bail!("TUN 文件描述符无效"); }
        let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
        if duplicate < 0 { bail!("无法复制 TUN 文件描述符"); }
        let owned = unsafe { OwnedFd::from_raw_fd(duplicate) };
        dispatch(|reply| Command::SetTun(owned, reply))
    })
}

#[no_mangle]
pub extern "system" fn Java_com_carriez_flutter_1hbb_EasyTierNative_stop(mut env: JNIEnv, _: JObject) -> jstring {
    output(&mut env, |_| dispatch(Command::Stop))
}

#[no_mangle]
pub extern "system" fn Java_com_carriez_flutter_1hbb_EasyTierNative_profile(mut env: JNIEnv, _: JObject, toml: JString, patch: JString) -> jstring {
    output(&mut env, |env| { let toml = input(env,toml,profile::MAX_TOML)?; let patch = input(env,patch,profile::MAX_TOML)?; profile::edit(&toml,&patch) })
}
