use super::Result;
#[cfg(unix)]
use hbb_common::config::Config;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    net::Ipv4Addr,
    path::{Path, PathBuf},
};
use toml_edit::{value, Array, ArrayOfTables, Document, Item, Table};
use uuid::Uuid;

pub(super) const MAX_TOML: usize = 128 * 1024;
const MAX_PROFILES: usize = 64;
const MAX_STORE: usize = 16 * 1024 * 1024;
const MAX_NAME: usize = 80;

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Profile {
    pub id: Uuid,
    pub name: String,
    pub toml: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub(super) struct Store {
    pub profiles: Vec<Profile>,
    pub selected_id: Option<Uuid>,
}

#[derive(Default, Deserialize)]
pub(super) struct Patch {
    network_name: Option<String>,
    secret: Option<String>,
    peers: Option<Vec<String>>,
    dhcp: Option<bool>,
    ipv4: Option<String>,
}

#[derive(Default, Serialize)]
pub(super) struct Fields {
    network_name: String,
    secret: String,
    peers: Vec<String>,
    dhcp: bool,
    ipv4: String,
}

fn private_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("找不到当前用户的本地应用数据目录。")?;
    #[cfg(target_os = "macos")]
    let base = Config::get_home().join("Library/Application Support");
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| Config::get_home().join(".config"));
    let path = base.join("SubnetDesk").join("easytier");

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&path).map_err(|_| "无法创建 EasyTier 方案目录。")?;

    let metadata = fs::symlink_metadata(&path).map_err(|_| "无法检查 EasyTier 方案目录。")?;
    if !metadata.is_dir() {
        return Err("EasyTier 方案目录不是实际目录。");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { hbb_common::libc::geteuid() } {
            return Err("EasyTier 方案目录不是当前用户专用目录。");
        }
    }
    Ok(path)
}

fn read_limited(file: File, max_bytes: usize) -> Result<String> {
    let mut bytes = Vec::new();
    file.take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取 EasyTier 方案文件。")?;
    if bytes.len() > max_bytes {
        return Err("EasyTier 方案文件过大。");
    }
    String::from_utf8(bytes).map_err(|_| "EasyTier 方案文件必须使用 UTF-8。")
}

pub(super) fn backup_store_path() -> Result<PathBuf> {
    Ok(private_dir()?.join("profiles.json"))
}

pub(super) fn validate_backup_store(data: &[u8]) -> Result<()> {
    if data.len() > MAX_STORE {
        return Err("EasyTier 方案的总长度超出限制。");
    }
    let store: Store = serde_json::from_slice(data).map_err(|_| "备份中的 EasyTier 方案索引无效。")?;
    if store.profiles.len() > MAX_PROFILES {
        return Err("备份中的 EasyTier 方案数量超出限制。");
    }
    let mut ids = std::collections::HashSet::new();
    for profile in &store.profiles {
        if !ids.insert(profile.id) {
            return Err("备份中的 EasyTier 方案 ID 重复。");
        }
        name(&profile.name)?;
        parse(&profile.toml)?;
    }
    if store.selected_id.is_some_and(|id| !ids.contains(&id)) {
        return Err("备份中选中的 EasyTier 方案不存在。");
    }
    Ok(())
}

pub(super) fn import(path: &Path) -> Result<String> {
    if !fs::metadata(path).map_err(|_| "无法检查待导入的 TOML 文件。")?.is_file() {
        return Err("导入来源必须是普通文件。");
    }
    let file = File::open(path).map_err(|_| "无法打开待导入的 TOML 文件。")?;
    if !file
        .metadata()
        .map_err(|_| "无法检查待导入的 TOML 文件。")?
        .is_file()
    {
        return Err("导入来源必须是普通文件。");
    }
    read_limited(file, MAX_TOML)
}

fn write_atomic(path: &Path, data: &[u8], overwrite: bool) -> Result<()> {
    let dir = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or("找不到 EasyTier 文件的上级目录。")?;
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|_| "无法创建 EasyTier 临时文件。")?;
    file.write_all(data)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| "无法写入 EasyTier 文件。")?;
    if overwrite {
        file.persist(path).map_err(|_| "无法保存 EasyTier 方案。")?;
    } else {
        file.persist_noclobber(path)
            .map_err(|_| "无法导出 EasyTier 方案，目标文件可能已存在。")?;
    }
    Ok(())
}

pub(super) fn export(path: &Path, toml: &str) -> Result<()> {
    write_atomic(path, toml.as_bytes(), false)
}

impl Store {
    pub fn open() -> Result<(PathBuf, Self)> {
        let path = private_dir()?.join("profiles.json");
        let mut store = match File::open(&path) {
            Ok(file) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if file.metadata().map_err(|_| "无法检查 EasyTier 方案。")?.mode() & 0o077 != 0 {
                        return Err("EasyTier 方案文件允许其他用户读取。");
                    }
                }
                serde_json::from_str::<Self>(&read_limited(file, MAX_STORE)?)
                    .map_err(|_| "无法读取 EasyTier 方案索引。")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(_) => return Err("无法打开 EasyTier 方案索引。"),
        };
        store.normalize_selection();
        Ok((path, store))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_vec(self).map_err(|_| "无法生成 EasyTier 方案索引。")?;
        if data.len() > MAX_STORE {
            return Err("EasyTier 方案的总长度超出限制。");
        }
        write_atomic(path, &data, true)
    }

    pub fn get(&self, id: Uuid) -> Result<&Profile> {
        self.profiles
            .iter()
            .find(|profile| profile.id == id)
            .ok_or("EasyTier 方案不存在。")
    }

    pub fn put(&mut self, profile: Profile) -> Result<()> {
        if let Some(existing) = self.profiles.iter_mut().find(|item| item.id == profile.id) {
            *existing = profile;
        } else {
            if self.profiles.len() >= MAX_PROFILES {
                return Err("EasyTier 方案数量已达上限。");
            }
            self.profiles.push(profile);
        }
        self.normalize_selection();
        Ok(())
    }

    pub fn normalize_selection(&mut self) {
        if !self.selected_id.is_some_and(|id| self.get(id).is_ok()) {
            self.selected_id = self.profiles.first().map(|profile| profile.id);
        }
    }
}

pub(super) fn name(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > MAX_NAME
        || trimmed.chars().any(char::is_control)
    {
        return Err("请输入有效的 EasyTier 方案名称，最多 80 个字符。");
    }
    Ok(trimmed.to_owned())
}

fn parse(toml: &str) -> Result<Document> {
    if toml.len() > MAX_TOML {
        return Err("EasyTier TOML 配置过大。");
    }
    toml.parse().map_err(|_| "EasyTier TOML 格式无效。")
}

pub(super) fn default_toml() -> String {
    format!(
        r#"dhcp = true
listeners = ["tcp://0.0.0.0:0", "udp://0.0.0.0:0", "quic://0.0.0.0:0"]

[network_identity]
network_name = "subnetdesk"
network_secret = "{}"

[flags]
multi_thread = true
latency_first = true
private_mode = true
enable_udp_broadcast_relay = false
enable_quic_proxy = false
disable_quic_input = true
accept_dns = false
disable_upnp = true
"#,
        Uuid::new_v4()
    )
}

fn peer_uris(item: Option<&Item>) -> Vec<String> {
    match item {
        Some(Item::ArrayOfTables(tables)) => tables
            .iter()
            .filter_map(|table| table.get("uri")?.as_str().map(str::to_owned))
            .collect(),
        Some(Item::Value(value)) => value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_inline_table()?.get("uri")?.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) fn fields(toml: &str) -> Fields {
    let Ok(doc) = parse(toml) else {
        return Fields::default();
    };
    let identity = doc.as_table().get("network_identity").and_then(Item::as_table_like);
    let string = |key| {
        identity
            .and_then(|table| table.get(key))
            .and_then(Item::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Fields {
        network_name: string("network_name"),
        secret: string("network_secret"),
        peers: peer_uris(doc.as_table().get("peer")),
        dhcp: doc.as_table().get("dhcp").and_then(Item::as_bool).unwrap_or_default(),
        ipv4: doc.as_table().get("ipv4").and_then(Item::as_str).unwrap_or_default().to_owned(),
    }
}

fn patch_peers(doc: &mut Document, peers: Vec<String>) {
    if peers == peer_uris(doc.as_table().get("peer")) {
        return;
    }
    if peers.is_empty() {
        doc.as_table_mut().remove("peer");
        return;
    }
    if let Some(array) = doc.as_table().get("peer").and_then(Item::as_array) {
        let mut result = Array::new();
        for peer in peers {
            let mut item = array
                .iter()
                .filter_map(|entry| entry.as_inline_table())
                .find(|table| table.get("uri").and_then(|item| item.as_str()) == Some(peer.as_str()))
                .cloned()
                .unwrap_or_default();
            item.insert("uri", toml_edit::Value::from(peer));
            result.push(toml_edit::Value::InlineTable(item));
        }
        doc.as_table_mut().insert("peer", value(result));
        return;
    }
    let old = doc.as_table().get("peer").and_then(Item::as_array_of_tables);
    let mut result = ArrayOfTables::new();
    for peer in peers {
        let mut table = old
            .into_iter()
            .flat_map(ArrayOfTables::iter)
            .find(|table| table.get("uri").and_then(Item::as_str) == Some(peer.as_str()))
            .cloned()
            .unwrap_or_default();
        table.insert("uri", value(peer));
        result.push(table);
    }
    doc.as_table_mut().insert("peer", Item::ArrayOfTables(result));
}

pub(super) fn patch(toml: &str, update: Option<Patch>) -> Result<String> {
    let mut doc = parse(toml)?;
    let Some(update) = update else {
        return Ok(toml.to_owned());
    };
    if update.network_name.is_some() || update.secret.is_some() {
        if doc.as_table().get("network_identity").is_none() {
            doc.as_table_mut().insert("network_identity", Item::Table(Table::new()));
        }
        let identity = doc
            .as_table_mut()
            .get_mut("network_identity")
            .and_then(Item::as_table_like_mut)
            .ok_or("EasyTier 的 network_identity 必须是表。")?;
        if let Some(network_name) = update.network_name {
            identity.insert("network_name", value(network_name));
        }
        if let Some(secret) = update.secret {
            identity.insert("network_secret", value(secret));
        }
    }
    if let Some(peers) = update.peers {
        patch_peers(&mut doc, peers);
    }
    if let Some(dhcp) = update.dhcp {
        doc.as_table_mut().insert("dhcp", value(dhcp));
    }
    if let Some(ipv4) = update.ipv4 {
        let ipv4 = ipv4.trim();
        if ipv4.is_empty() {
            doc.as_table_mut().remove("ipv4");
        } else {
            let (addr, prefix) = ipv4.split_once('/').unwrap_or((ipv4, "24"));
            if addr.parse::<Ipv4Addr>().is_err()
                || !prefix.parse::<u8>().is_ok_and(|prefix| prefix <= 32)
            {
                return Err("固定虚拟 IP 必须是有效的 IPv4 地址。");
            }
            doc.as_table_mut().insert("ipv4", value(format!("{addr}/{prefix}")));
        }
    }
    let patched = doc.to_string();
    if patched.len() > MAX_TOML {
        return Err("EasyTier TOML 配置过大。");
    }
    Ok(patched)
}

pub(super) fn launch_toml(toml: &str, instance_id: Uuid, hostname: &str) -> Result<String> {
    let mut doc = parse(toml)?;
    // 运行用 ID 与方案 ID 解耦，防止把同端口上的其他实例误认成当前实例。
    doc.as_table_mut().insert("instance_id", value(instance_id.to_string()));
    // 节点统一使用「本机」显示名称，方案文件中的 hostname 不覆盖应用名称。
    doc.as_table_mut().insert("hostname", value(hostname));
    Ok(doc.to_string())
}
