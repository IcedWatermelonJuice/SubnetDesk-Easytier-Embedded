//! Android 主进程接收私有 VPN 服务快照；仅用于入站来源 IP 限制。
use hbb_common::{anyhow::{anyhow, bail}, config::Config, ResultType};
use jni::{objects::{JClass, JString}, JNIEnv};
use serde::Deserialize;
use std::{collections::HashSet, net::{IpAddr, Ipv4Addr}, sync::Mutex, time::{Duration, Instant}};

#[path = "easytier/source_policy.rs"]
mod source_policy;

const MAX_SNAPSHOT_BYTES: usize = 512 * 1024;
const MAX_NODES: usize = 2048;
const MAX_AGE_MS: i64 = 15_000;

#[derive(Default)]
struct Sources {
    addresses: HashSet<IpAddr>,
    observed_at: Option<Instant>,
}

static SOURCES: Mutex<Option<Sources>> = Mutex::new(None);
static REMOTE_ONLY: Mutex<Option<bool>> = Mutex::new(None);

#[derive(Deserialize)]
struct Node { address: String }

#[derive(Deserialize)]
struct Snapshot {
    state: String,
    #[serde(default)]
    remote_only: Option<bool>,
    #[serde(default)]
    queried_at: Option<i64>,
    #[serde(default)]
    nodes: Vec<Node>,
}

fn invalidate() {
    *SOURCES.lock().unwrap() = None;
}

pub(crate) fn sync_configuration() {
    // Application 可能先于 Flutter 初始化；不能在 APP_DIR 确定前加载配置缓存。
    if hbb_common::config::APP_DIR.read().unwrap().is_empty() { return; }
    let enabled = *REMOTE_ONLY.lock().unwrap();
    if let Some(enabled) = enabled {
        let key = base::config::keys::OPTION_LAN_EASYTIER_ONLY;
        if (Config::get_option(key) == "Y") != enabled {
            Config::set_option(key.to_owned(), if enabled { "Y" } else { "" }.to_owned());
        }
    }
}

pub(crate) fn remote_only() -> Option<bool> {
    sync_configuration();
    *REMOTE_ONLY.lock().unwrap()
}

fn accept_snapshot(encoded: &str) -> ResultType<()> {
    if encoded.len() > MAX_SNAPSHOT_BYTES { bail!("Android EasyTier 状态超过容量限制"); }
    let snapshot: Snapshot = serde_json::from_str(encoded)?;
    if snapshot.nodes.len() > MAX_NODES { bail!("Android EasyTier 节点数量超过限制"); }
    if let Some(enabled) = snapshot.remote_only {
        *REMOTE_ONLY.lock().unwrap() = Some(enabled);
        sync_configuration();
    }
    let age = snapshot.queried_at
        .map(|time| hbb_common::get_time().saturating_sub(time))
        .filter(|age| (0..=MAX_AGE_MS).contains(age));
    if snapshot.state != "running" || age.is_none() {
        invalidate();
        return Ok(());
    }
    let addresses = snapshot.nodes.iter().filter_map(|node| node.address.parse::<Ipv4Addr>().ok())
        .filter(|ip| !ip.is_unspecified() && !ip.is_loopback() && !ip.is_multicast() && !ip.is_broadcast())
        .map(IpAddr::V4).collect();
    let observed_at = age.and_then(|age| Instant::now().checked_sub(Duration::from_millis(age as u64)));
    *SOURCES.lock().unwrap() = Some(Sources { addresses, observed_at });
    Ok(())
}

pub(crate) fn source_allowed(ip: IpAddr) -> bool {
    let sources = SOURCES.lock().unwrap();
    sources.as_ref().is_some_and(|sources|
        source_policy::accepts(&sources.addresses, sources.observed_at, ip))
}

/// 由主进程 Application 的受签名权限保护接收器调用，不依赖 Flutter 页面存活。
#[no_mangle]
pub extern "system" fn Java_ffi_FFI_onEasyTierState(mut env: JNIEnv, _class: JClass, value: JString) {
    let result = (|| -> ResultType<()> {
        let value: String = env.get_string(&value)
            .map_err(|_| anyhow!("无法读取 Android EasyTier 状态"))?.into();
        accept_snapshot(&value)
    })();
    if let Err(error) = result {
        // 无效状态只能收紧来源限制，不能清除用户选择的仅 EasyTier 模式。
        invalidate();
        hbb_common::log::warn!("无法更新 Android EasyTier 来源快照: {error}");
    }
    match env.exception_check() {
        Ok(true) => if let Err(error) = env.exception_clear() {
            hbb_common::log::warn!("无法清除 Android EasyTier JNI 异常: {error}");
        },
        Ok(false) => {},
        Err(error) => hbb_common::log::warn!("无法检查 Android EasyTier JNI 异常: {error}"),
    }
}
