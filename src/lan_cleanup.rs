const OFFLINE_RETENTION_MS: i64 = 30 * 60 * 1_000;

pub(crate) fn expired(online: Option<bool>, last_seen: i64, now: i64) -> bool {
    online == Some(false) && last_seen > 0
        && now.saturating_sub(last_seen) > OFFLINE_RETENTION_MS
}

pub(crate) fn rediscovered(removed_at: i64, online: Option<bool>, last_seen: i64, last_connected: i64) -> bool {
    last_connected > removed_at || (online == Some(true) && last_seen > removed_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_requires_confirmed_offline_for_more_than_half_an_hour() {
        let seen = 1_000;
        assert!(!expired(Some(false), seen, seen + OFFLINE_RETENTION_MS));
        assert!(expired(Some(false), seen, seen + OFFLINE_RETENTION_MS + 1));
        assert!(!expired(Some(true), seen, seen + OFFLINE_RETENTION_MS + 1));
        assert!(!expired(None, seen, seen + OFFLINE_RETENTION_MS + 1));
        assert!(!expired(Some(false), 0, OFFLINE_RETENTION_MS + 1));
    }

    #[test]
    fn removal_is_not_reversed_by_an_old_cached_online_result() {
        assert!(!rediscovered(1_000, Some(true), 999, 900));
        assert!(!rediscovered(1_000, Some(true), 1_000, 900));
        assert!(!rediscovered(1_000, Some(false), 1_001, 900));
        assert!(rediscovered(1_000, Some(true), 1_001, 900));
        assert!(rediscovered(1_000, None, 0, 1_001));
    }
}
