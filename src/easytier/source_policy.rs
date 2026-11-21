use std::{collections::HashSet, net::IpAddr, time::{Duration, Instant}};

// 查询数据过期时拒绝新连接，不能把上一次组网的节点继续作为允许来源。
pub(crate) fn accepts(sources: &HashSet<IpAddr>, observed_at: Option<Instant>, ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(ip) => ip.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(ip)),
        ip => ip,
    };
    observed_at.is_some_and(|time| time.elapsed() <= Duration::from_secs(15))
        && sources.contains(&ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_node_ips_are_accepted() {
        let sources = HashSet::from(["10.88.0.2".parse().unwrap()]);
        let now = Some(Instant::now());
        assert!(accepts(&sources, now, "10.88.0.2".parse().unwrap()));
        assert!(accepts(&sources, now, "::ffff:10.88.0.2".parse().unwrap()));
        assert!(!accepts(&sources, now, "10.88.0.3".parse().unwrap()));
        assert!(!accepts(&sources, now, "192.168.1.2".parse().unwrap()));
        assert!(!accepts(&sources, now, "::1".parse().unwrap()));
    }

    #[test]
    fn unavailable_or_expired_queries_reject_connections() {
        let ip = "10.88.0.2".parse().unwrap();
        let sources = HashSet::from([ip]);
        assert!(!accepts(&sources, None, ip));
        assert!(!accepts(&HashSet::new(), Some(Instant::now()), ip));
        assert!(!accepts(&sources, Some(Instant::now() - Duration::from_secs(16)), ip));
    }
}
