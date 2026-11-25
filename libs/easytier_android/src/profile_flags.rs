//! Android 方案始终使用应用提供的 TUN，不启用桌面专用转发能力。
use anyhow::{anyhow, Result};
use toml_edit::{value, DocumentMut, Item, Table};

pub(crate) fn normalize(doc: &mut DocumentMut, latency_first: Option<bool>) -> Result<()> {
    if doc.get("flags").is_none() {
        doc["flags"] = Item::Table(Table::new());
    }
    let flags = doc.get_mut("flags").and_then(Item::as_table_like_mut)
        .ok_or_else(|| anyhow!("flags 必须是表"))?;
    for key in ["proxy_forward_by_system", "no_tun", "enable_udp_broadcast_relay"] {
        flags.insert(key, value(false));
    }
    // TCP 加速代理会重建本地连接，无法直接校验原始节点 IP。
    for (key, enabled) in [
        ("disable_quic_input", true), ("disable_kcp_input", true),
        ("enable_quic_proxy", false), ("enable_kcp_proxy", false),
    ] {
        flags.insert(key, value(enabled));
    }
    if let Some(enabled) = latency_first {
        flags.insert("latency_first", value(enabled));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_flags_are_overridden_without_losing_other_settings() {
        for flags in [
            "[flags]\nproxy_forward_by_system = true\nno_tun = true\nenable_udp_broadcast_relay = true\nmtu = 1380\n",
            "flags = { proxy_forward_by_system = true, no_tun = true, enable_udp_broadcast_relay = true, mtu = 1380 }\n",
        ] {
            let mut doc: DocumentMut = flags.parse().unwrap();
            normalize(&mut doc, None).unwrap();
            for key in ["proxy_forward_by_system", "no_tun", "enable_udp_broadcast_relay"] {
                assert_eq!(doc["flags"][key].as_bool(), Some(false));
            }
            assert_eq!(doc["flags"]["mtu"].as_integer(), Some(1380));
        }
    }

    #[test]
    fn missing_flags_are_added_and_normalization_is_idempotent() {
        let mut doc: DocumentMut = "dhcp = true\n[network_identity]\nnetwork_name = 'test'\n".parse().unwrap();
        normalize(&mut doc, None).unwrap();
        let once = doc.to_string();
        normalize(&mut doc, None).unwrap();
        assert_eq!(doc.to_string(), once);
        assert_eq!(doc["dhcp"].as_bool(), Some(true));
        assert_eq!(doc["network_identity"]["network_name"].as_str(), Some("test"));
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;

    #[test]
    fn proxy_override_and_latency_round_trip() {
        for source in [
            "[flags]\nenable_quic_proxy = true\nenable_kcp_proxy = true\ndisable_quic_input = false\ndisable_kcp_input = false\nmtu = 1380\n",
            "flags = { enable_quic_proxy = true, enable_kcp_proxy = true, disable_quic_input = false, disable_kcp_input = false, mtu = 1380 }\n",
        ] {
            let mut doc: DocumentMut = source.parse().unwrap();
            for enabled in [true, false] {
                normalize(&mut doc, Some(enabled)).unwrap();
                assert_eq!(doc["flags"]["latency_first"].as_bool(), Some(enabled));
                for key in ["disable_quic_input", "disable_kcp_input"] {
                    assert_eq!(doc["flags"][key].as_bool(), Some(true));
                }
                for key in ["enable_quic_proxy", "enable_kcp_proxy"] {
                    assert_eq!(doc["flags"][key].as_bool(), Some(false));
                }
                assert_eq!(doc["flags"]["mtu"].as_integer(), Some(1380));
                normalize(&mut doc, None).unwrap();
                assert_eq!(doc["flags"]["latency_first"].as_bool(), Some(enabled));
            }
        }
    }
}
