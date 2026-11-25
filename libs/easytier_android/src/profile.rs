//! 与桌面编辑字段保持一致；不以字符串替换或正则改写 TOML。
use anyhow::{anyhow, bail, Result};
use easytier::common::config::TomlConfigLoader;
use serde::Deserialize;
use serde_json::{json, Value};
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, Item, Table};

pub(crate) const MAX_TOML: usize = 128 * 1024;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    hostname: Option<String>,
    network_name: Option<String>,
    secret: Option<String>,
    peers: Option<Vec<String>>,
    dhcp: Option<bool>,
    latency_first: Option<bool>,
    ipv4: Option<String>,
}

fn default_toml() -> String {
    format!(r#"dhcp = true
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
enable_kcp_proxy = false
disable_kcp_input = true
accept_dns = false
disable_upnp = true
bind_device = false
"#, uuid::Uuid::new_v4())
}

fn peers(doc: &DocumentMut) -> Vec<String> {
    match doc.get("peer") {
        Some(Item::ArrayOfTables(tables)) => tables.iter().filter_map(|t| t.get("uri")?.as_str().map(str::to_owned)).collect(),
        Some(Item::Value(array)) => array.as_array().into_iter().flatten()
            .filter_map(|v| v.as_inline_table()?.get("uri")?.as_str().map(str::to_owned)).collect(),
        _ => vec![],
    }
}

fn patch_peers(doc: &mut DocumentMut, next: Vec<String>) {
    if next == peers(doc) { return; }
    if next.is_empty() { doc.as_table_mut().remove("peer"); return; }
    if let Some(array) = doc.get("peer").and_then(Item::as_array) {
        let mut result = Array::new();
        for uri in next {
            let mut table = array.iter().filter_map(|v| v.as_inline_table())
                .find(|t| t.get("uri").and_then(|v| v.as_str()) == Some(uri.as_str())).cloned().unwrap_or_default();
            table.insert("uri", toml_edit::Value::from(uri));
            result.push(toml_edit::Value::InlineTable(table));
        }
        doc["peer"] = value(result);
    } else {
        let old = doc.get("peer").and_then(Item::as_array_of_tables);
        let mut result = ArrayOfTables::new();
        for uri in next {
            let mut table = old.into_iter().flat_map(ArrayOfTables::iter)
                .find(|t| t.get("uri").and_then(Item::as_str) == Some(uri.as_str())).cloned().unwrap_or_default();
            table.insert("uri", value(uri));
            result.push(table);
        }
        doc["peer"] = Item::ArrayOfTables(result);
    }
}

pub(crate) fn validate(toml: &str) -> Result<()> {
    if toml.is_empty() || toml.len() > MAX_TOML { bail!("方案为空或超过 128 KiB 限制"); }
    // 上游解析错误包含原始 TOML，不能把含密钥的错误文本发给界面或日志。
    TomlConfigLoader::new_from_str(toml).map_err(|_| anyhow!("EasyTier TOML 配置无效"))?;
    Ok(())
}

pub(crate) fn edit(toml: &str, patch: &str) -> Result<Value> {
    if toml.len() > MAX_TOML || patch.len() > MAX_TOML { bail!("方案内容过大"); }
    let original = if toml.is_empty() { default_toml() } else { toml.to_owned() };
    let mut doc: DocumentMut = original.parse().map_err(|_| anyhow!("EasyTier TOML 格式无效"))?;
    let patch: Patch = serde_json::from_str(patch).map_err(|_| anyhow!("方案编辑字段无效"))?;
    if let Some(hostname) = patch.hostname {
        let hostname: String = hostname.trim().chars().filter(|c| !c.is_control()).take(64).collect();
        if hostname.is_empty() { bail!("设备名称不能为空"); }
        doc["hostname"] = value(hostname);
    }
    if patch.network_name.is_some() || patch.secret.is_some() {
        if doc.get("network_identity").is_none() { doc["network_identity"] = Item::Table(Table::new()); }
        let identity = doc.get_mut("network_identity").and_then(Item::as_table_like_mut)
            .ok_or_else(|| anyhow!("network_identity 必须是表"))?;
        if let Some(name) = patch.network_name { identity.insert("network_name", value(name)); }
        if let Some(secret) = patch.secret { identity.insert("network_secret", value(secret)); }
    }
    if let Some(peers) = patch.peers { patch_peers(&mut doc, peers); }
    if let Some(dhcp) = patch.dhcp { doc["dhcp"] = value(dhcp); }
    if let Some(ipv4) = patch.ipv4 {
        if ipv4.trim().is_empty() { doc.as_table_mut().remove("ipv4"); }
        else {
            let (ip, prefix) = ipv4.trim().split_once('/').unwrap_or((ipv4.trim(), "24"));
            if ip.parse::<std::net::Ipv4Addr>().is_err() || !prefix.parse::<u8>().is_ok_and(|p| p <= 32) {
                bail!("固定虚拟 IP 必须是有效的 IPv4 地址");
            }
            doc["ipv4"] = value(format!("{ip}/{prefix}"));
        }
    }
    super::profile_flags::normalize(&mut doc, patch.latency_first)?;
    let output = doc.to_string();
    validate(&output)?;
    let identity = doc.get("network_identity").and_then(Item::as_table_like);
    let field = |key| identity.and_then(|t| t.get(key)).and_then(Item::as_str).unwrap_or_default();
    Ok(json!({"ok":true,"toml":output,"fields":{
        "network_name":field("network_name"),"secret":field("network_secret"),"peers":peers(&doc),
        "latency_first":doc.get("flags").and_then(Item::as_table_like)
            .and_then(|flags| flags.get("latency_first")).and_then(Item::as_bool).unwrap_or(false),
        "dhcp":doc.get("dhcp").and_then(Item::as_bool).unwrap_or(false),
        "ipv4":doc.get("ipv4").and_then(Item::as_str).unwrap_or_default()
    }}))
}
