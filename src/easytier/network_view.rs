//! 将同一轮 CLI 节点、路由结果关联为展示数据，不参与访问控制或实际选路。
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, net::Ipv4Addr};

#[derive(Serialize)]
pub(crate) struct Node {
    id: String,
    name: String,
    address: String,
    latency: Option<f64>,
    protocol: Option<String>,
    path_kind: &'static str,
    hops: Option<i32>,
    path_latency: Option<f64>,
    next_hop_name: String,
    next_hop_ip: String,
    next_hop_latency: Option<f64>,
    path_strategy: &'static str,
    direct_available: bool,
    direct_latency: Option<f64>,
    proxy_cidrs: Vec<String>,
    loss: String,
    rx_bytes: String,
    tx_bytes: String,
    nat_type: String,
    version: String,
}

#[derive(Default, Deserialize)]
pub(super) struct PeerRow {
    #[serde(default)]
    pub(super) id: String,
    pub(super) hostname: String,
    pub(super) ipv4: String,
    pub(super) cidr: String,
    pub(super) cost: String,
    pub(super) lat_ms: String,
    pub(super) tunnel_proto: String,
    #[serde(default)]
    loss_rate: String,
    #[serde(default)]
    rx_bytes: String,
    #[serde(default)]
    tx_bytes: String,
    #[serde(default)]
    nat_type: String,
    #[serde(default)]
    version: String,
}

#[derive(Default, Deserialize)]
pub(super) struct RouteRow {
    pub(super) ipv4: String,
    #[serde(default)]
    hostname: String,
    pub(super) proxy_cidrs: String,
    pub(super) next_hop_ipv4: String,
    pub(super) next_hop_hostname: String,
    pub(super) path_len: i32,
    pub(super) path_latency: i32,
    pub(super) next_hop_ipv4_lat_first: String,
    pub(super) next_hop_hostname_lat_first: String,
    pub(super) path_len_lat_first: i32,
    pub(super) path_latency_lat_first: i32,
}

fn text(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).take(128).collect::<String>().trim().to_owned()
}

fn reported_text(value: &str) -> String {
    let value = text(value);
    if value == "-" || value.eq_ignore_ascii_case("unknown") { String::new() } else { value }
}

fn ip(value: &str) -> Option<String> {
    let (address, prefix) = match value.trim().split_once('/') {
        Some((address, prefix)) => (address, Some(prefix)),
        None => (value.trim(), None),
    };
    if prefix.is_some_and(|prefix| prefix.parse::<u8>().map_or(true, |bits| bits > 32)) {
        return None;
    }
    address.parse::<Ipv4Addr>().ok()
        .filter(|address| !address.is_unspecified() && !address.is_loopback())
        .map(|address| address.to_string())
}

fn positive(value: f64) -> Option<f64> {
    // CLI 多处把缺失的统计写成 0，不能把它显示成真实的零延迟。
    (value.is_finite() && value > 0.0).then_some(value)
}

fn direct(peer: &PeerRow) -> bool {
    peer.cost == "p2p"
}

fn direct_latency(peer: &PeerRow) -> Option<f64> {
    direct(peer).then(|| peer.lat_ms.parse::<f64>().ok().and_then(positive)).flatten()
}

fn unique_peer<'a>(peers: &'a [PeerRow], address: &str) -> Option<&'a PeerRow> {
    let mut matches = peers.iter().filter(|peer| ip(&peer.ipv4).as_deref() == Some(address));
    let peer = matches.next()?;
    matches.next().is_none().then_some(peer)
}

fn selected(route: &RouteRow, latency_first: bool) -> (&str, &str, i32, i32, &'static str) {
    if latency_first && route.path_len_lat_first > 0 {
        (&route.next_hop_ipv4_lat_first, &route.next_hop_hostname_lat_first,
            route.path_len_lat_first, route.path_latency_lat_first, "latency_first")
    } else {
        (&route.next_hop_ipv4, &route.next_hop_hostname,
            route.path_len, route.path_latency,
            if latency_first { "least_hops_fallback" } else { "least_hops" })
    }
}

fn matching_names(left: &str, right: &str) -> bool {
    let left = reported_text(left);
    let right = reported_text(right);
    left.is_empty() || right.is_empty() || left == right
}

pub(super) fn build_nodes(peers: &[PeerRow], routes: &[RouteRow], latency_first: bool, max_rows: usize) -> Vec<Node> {
    peers.iter().filter(|peer| peer.cost != "Local").take(max_rows).map(|peer| {
        let address = ip(&peer.ipv4).unwrap_or_default();
        let direct_available = direct(peer);
        let direct_delay = direct_latency(peer);
        let mut node = Node {
            id: reported_text(&peer.id), name: text(&peer.hostname), address: address.clone(),
            // 旧入口也使用所选路径的延迟，避免三个页面口径不同。
            latency: None,
            protocol: match reported_text(&peer.tunnel_proto) { value if value.is_empty() => None, value => Some(value) },
            path_kind: "unknown", hops: None, path_latency: None,
            next_hop_name: String::new(), next_hop_ip: String::new(), next_hop_latency: None,
            path_strategy: if latency_first { "latency_first" } else { "least_hops" },
            direct_available, direct_latency: direct_delay, proxy_cidrs: Vec::new(),
            loss: if direct_available { reported_text(&peer.loss_rate) } else { String::new() },
            rx_bytes: if direct_available { reported_text(&peer.rx_bytes) } else { String::new() },
            tx_bytes: if direct_available { reported_text(&peer.tx_bytes) } else { String::new() },
            nat_type: reported_text(&peer.nat_type), version: reported_text(&peer.version),
        };
        if node.id.is_empty() { node.id = address.clone(); }
        // 没有目标 IP、重复 IP 或两次查询的名称矛盾时，不拼接可能属于另一台设备的路由。
        if address.is_empty() || unique_peer(peers, &address).is_none() { return node; }
        let mut matching = routes.iter().filter(|route| ip(&route.ipv4).as_deref() == Some(address.as_str()));
        let Some(route) = matching.next() else { return node; };
        if matching.next().is_some() || !matching_names(&peer.hostname, &route.hostname) { return node; }
        if node.name.is_empty() { node.name = text(&route.hostname); }
        let mut subnets = HashSet::new();
        node.proxy_cidrs = route.proxy_cidrs.split(',').map(text)
            .filter(|cidr| !cidr.is_empty() && cidr != "-" && subnets.insert(cidr.clone())).take(max_rows).collect();
        let (hop_ip, hop_name, hops, latency, strategy) = selected(route, latency_first);
        node.path_strategy = strategy;
        let is_direct = hop_ip == "DIRECT" && (hop_name.is_empty() || hop_name == "DIRECT");
        if hops == 1 && is_direct && direct_available {
            node.path_kind = "direct";
            node.hops = Some(1);
            node.path_latency = direct_delay;
            node.next_hop_name = node.name.clone();
            node.next_hop_ip = address;
            node.next_hop_latency = direct_delay;
        } else if hops > 1 && !is_direct {
            let Some(hop_ip) = ip(hop_ip).filter(|hop| hop != &address) else { return node; };
            let Some(hop) = unique_peer(peers, &hop_ip).filter(|hop| direct(hop)) else { return node; };
            if !matching_names(hop_name, &hop.hostname) { return node; }
            node.path_kind = "relay";
            node.hops = Some(hops);
            node.path_latency = positive(f64::from(latency));
            node.next_hop_name = if reported_text(hop_name).is_empty() { text(&hop.hostname) } else { text(hop_name) };
            node.next_hop_ip = hop_ip;
            node.next_hop_latency = direct_latency(hop);
        }
        node.latency = node.path_latency;
        node
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str, address: &str, name: &str, connected: bool) -> PeerRow {
        PeerRow { id: id.into(), ipv4: address.into(), hostname: name.into(),
            cost: if connected { "p2p" } else { "relay(2)" }.into(),
            tunnel_proto: if connected { "udp" } else { "-" }.into(), lat_ms: "12.34".into(), ..Default::default() }
    }

    fn route(address: &str) -> RouteRow {
        RouteRow { ipv4: address.into(), next_hop_ipv4: "DIRECT".into(), next_hop_hostname: "DIRECT".into(),
            path_len: 1, path_latency: 12, ..Default::default() }
    }

    #[test]
    fn direct_route_and_cidr_addresses_are_associated() {
        let nodes = build_nodes(&[peer("1", "10.0.0.2", "Office", true)], &[route("10.0.0.2/24")], false, 100);
        assert_eq!(nodes[0].path_kind, "direct");
        assert_eq!(nodes[0].path_latency, Some(12.34));
        assert_eq!(nodes[0].latency, nodes[0].path_latency);
        assert_eq!(nodes[0].next_hop_name, "Office");
        assert_eq!(nodes[0].path_strategy, "least_hops");
    }

    #[test]
    fn latency_first_can_select_relay_despite_an_available_direct_link() {
        let mut target_route = route("10.0.0.3/24");
        target_route.next_hop_ipv4_lat_first = "10.0.0.2/24".into();
        target_route.next_hop_hostname_lat_first = "Relay".into();
        target_route.path_len_lat_first = 2;
        target_route.path_latency_lat_first = 8;
        let nodes = build_nodes(&[peer("2", "10.0.0.3", "Target", true), peer("1", "10.0.0.2", "Relay", true)], &[target_route], true, 100);
        assert_eq!(nodes[0].path_kind, "relay");
        assert!(nodes[0].direct_available);
        assert_eq!(nodes[0].direct_latency, Some(12.34));
        assert_eq!(nodes[0].path_latency, Some(8.0));
        assert_eq!(nodes[0].next_hop_name, "Relay");
        assert_eq!(nodes[0].next_hop_ip, "10.0.0.2");
        assert_eq!(nodes[0].next_hop_latency, Some(12.34));
    }

    #[test]
    fn relay_uses_selected_strategy_and_preserves_subnets() {
        let relay_route = RouteRow { ipv4: "10.0.0.3".into(), next_hop_ipv4: "10.0.0.2".into(),
            next_hop_hostname: "Relay".into(), path_len: 2, path_latency: 36,
            proxy_cidrs: "192.168.1.0/24,192.168.1.0/24".into(), ..Default::default() };
        let nodes = build_nodes(&[peer("2", "10.0.0.3", "Target", false), peer("1", "10.0.0.2", "Relay", true)], &[relay_route], true, 100);
        assert_eq!(nodes[0].path_kind, "relay");
        assert_eq!(nodes[0].path_strategy, "least_hops_fallback");
        assert_eq!(nodes[0].path_latency, Some(36.0));
        assert_eq!(nodes[0].proxy_cidrs, ["192.168.1.0/24"]);
        assert!(!nodes[0].direct_available);
    }

    #[test]
    fn missing_conflicting_and_unknown_values_do_not_claim_direct_or_zero_latency() {
        let mut target = peer("1", "10.0.0.2", "Office", true);
        target.lat_ms = "0.00".into();
        target.tunnel_proto = "-".into();
        let nodes = build_nodes(&[target], &[route("10.0.0.2")], false, 100);
        assert_eq!(nodes[0].path_kind, "direct");
        assert_eq!(nodes[0].path_latency, None);
        let target = peer("1", "10.0.0.2", "Office", true);
        assert_eq!(build_nodes(&[target], &[], false, 100)[0].path_kind, "unknown");
        let mut wrong = route("10.0.0.2"); wrong.hostname = "Different device".into();
        assert_eq!(build_nodes(&[peer("1", "10.0.0.2", "Office", true)], &[wrong], false, 100)[0].path_kind, "unknown");
        assert_eq!(build_nodes(&[peer("1", "10.0.0.2", "Office", true), peer("2", "10.0.0.2", "Other", true)], &[route("10.0.0.2")], false, 100)[0].path_kind, "unknown");
    }

    #[test]
    fn display_fields_are_optional_but_existing_query_requirements_remain_strict() {
        let legacy = r#"{"hostname":"Office","ipv4":"10.0.0.2","cidr":"10.0.0.2/24","cost":"p2p","lat_ms":"12","tunnel_proto":"-"}"#;
        assert!(serde_json::from_str::<PeerRow>(legacy).is_ok());
        assert!(serde_json::from_str::<PeerRow>(r#"{"cost":"Local"}"#).is_err());
    }
}
