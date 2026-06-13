// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use {
    hickory_resolver::{proto::rr::RData, TokioResolver},
    reqwest,
    tracing::{debug, warn},
};
use std::{
    collections::HashMap,
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MtaStsMode {
    Enforce,
    Testing,
    /// Policy exists but is inactive.
    None,
}

#[derive(Debug, Clone)]
pub struct MtaStsPolicy {
    pub mode: MtaStsMode,
    /// MX hostname patterns from the policy (e.g. "*.example.com").
    pub mx: Vec<String>,
    pub max_age: u64,
}

struct CachedPolicy {
    policy: MtaStsPolicy,
    fetched_at: Instant,
    /// The `id=` value from the _mta-sts TXT record at fetch time.
    policy_id: String,
}

static POLICY_CACHE: OnceLock<RwLock<HashMap<String, CachedPolicy>>> = OnceLock::new();

fn policy_cache() -> &'static RwLock<HashMap<String, CachedPolicy>> {
    POLICY_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Extract the `id=` value from the `_mta-sts` TXT record for `domain`.
async fn lookup_policy_id(domain: &str, resolver: &TokioResolver) -> Option<String> {
    let txt_name = format!("_mta-sts.{domain}");
    let txt_resp = resolver.txt_lookup(txt_name.as_str()).await.ok()?;

    for record in txt_resp.answers() {
        if let RData::TXT(txt) = &record.data {
            let combined: String = txt
                .txt_data
                .iter()
                .map(|chunk| String::from_utf8_lossy(chunk))
                .collect();
            if combined.starts_with("v=STSv1") {
                for part in combined.split(';') {
                    let part = part.trim();
                    if let Some(id) = part.strip_prefix("id=") {
                        return Some(id.trim().to_string());
                    }
                }
            }
        }
    }
    None
}

/// Fetch the raw policy text over HTTPS (RFC 8461 §3.3).
#[allow(clippy::cognitive_complexity)]
async fn fetch_policy_text(domain: &str) -> Option<String> {
    let url = format!("https://mta-sts.{domain}/.well-known/mta-sts.txt");
    debug!("MTA-STS: fetching policy from {url}");
    match reqwest::get(&url).await {
        Ok(resp) => match resp.text().await {
            Ok(t) => Some(t),
            Err(e) => {
                warn!("MTA-STS: failed to read policy body for {domain}: {e}");
                None
            }
        },
        Err(e) => {
            warn!("MTA-STS: failed to fetch policy for {domain}: {e}");
            None
        }
    }
}

/// Fetch and cache the MTA-STS policy for `domain`.
/// Returns `None` if the domain has no policy or the policy cannot be parsed.
pub async fn fetch_mta_sts_policy(
    domain: &str,
    resolver: &TokioResolver,
) -> Option<MtaStsPolicy> {
    let policy_id = lookup_policy_id(domain, resolver).await?;

    // Check the in-process cache before making an HTTPS request.
    {
        let cache = policy_cache().read().await;
        if let Some(cached) = cache.get(domain) {
            if cached.policy_id == policy_id
                && cached.fetched_at.elapsed() < Duration::from_secs(cached.policy.max_age)
            {
                debug!("MTA-STS: using cached policy for {domain}");
                return Some(cached.policy.clone());
            }
        }
    }

    let body = fetch_policy_text(domain).await?;
    let policy = parse_mta_sts_policy(&body)?;
    debug!(
        "MTA-STS: parsed policy for {domain}: mode={:?} mx={:?}",
        policy.mode, policy.mx
    );

    {
        let mut cache = policy_cache().write().await;
        cache.insert(
            domain.to_string(),
            CachedPolicy {
                policy: policy.clone(),
                fetched_at: Instant::now(),
                policy_id,
            },
        );
    }

    Some(policy)
}

fn parse_mta_sts_policy(body: &str) -> Option<MtaStsPolicy> {
    let mut version: Option<&str> = None;
    let mut mode: Option<MtaStsMode> = None;
    let mut mx: Vec<String> = Vec::new();
    let mut max_age: Option<u64> = None;

    for line in body.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            match key.trim() {
                "version" => version = Some(value.trim()),
                "mode" => {
                    mode = Some(match value.trim() {
                        "enforce" => MtaStsMode::Enforce,
                        "testing" => MtaStsMode::Testing,
                        _ => MtaStsMode::None,
                    });
                }
                "mx" => mx.push(value.trim().to_string()),
                "max_age" => max_age = value.trim().parse::<u64>().ok(),
                _ => {}
            }
        }
    }

    if version != Some("STSv1") {
        warn!("MTA-STS: policy has unsupported version: {version:?}");
        return None;
    }

    Some(MtaStsPolicy {
        mode: mode.unwrap_or(MtaStsMode::None),
        mx,
        max_age: max_age.unwrap_or(86400),
    })
}

/// Returns true if `host` matches the MTA-STS pattern.
/// Patterns may be exact ("mail.example.com") or wildcard ("*.example.com").
/// A wildcard matches exactly one DNS label on the left.
pub fn mx_matches_pattern(host: &str, pattern: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix("*.") {
        // Wildcard: the host must end with ".{suffix}" and the remaining
        // prefix must be a single label (no dots).
        let dot_suffix = format!(".{suffix}");
        if let Some(prefix) = host.to_lowercase().strip_suffix(&dot_suffix.to_lowercase()) {
            return !prefix.is_empty() && !prefix.contains('.');
        }
        false
    } else {
        host.eq_ignore_ascii_case(pattern)
    }
}

/// Returns true if `host` is permitted by the policy's MX list.
pub fn mx_allowed_by_policy(host: &str, policy: &MtaStsPolicy) -> bool {
    policy.mx.iter().any(|p| mx_matches_pattern(host, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_matches_single_label() {
        assert!(mx_matches_pattern("mail.example.com", "*.example.com"));
        assert!(mx_matches_pattern("smtp.example.com", "*.example.com"));
    }

    #[test]
    fn wildcard_does_not_match_multiple_labels() {
        assert!(!mx_matches_pattern("a.b.example.com", "*.example.com"));
    }

    #[test]
    fn wildcard_does_not_match_bare_suffix() {
        assert!(!mx_matches_pattern("example.com", "*.example.com"));
    }

    #[test]
    fn exact_match() {
        assert!(mx_matches_pattern("mail.example.com", "mail.example.com"));
        assert!(mx_matches_pattern("MAIL.EXAMPLE.COM", "mail.example.com"));
    }

    #[test]
    fn exact_no_match() {
        assert!(!mx_matches_pattern("smtp.example.com", "mail.example.com"));
    }

    #[test]
    fn parse_enforce_policy() {
        let body = "version: STSv1\nmode: enforce\nmx: *.example.com\nmx: mail.example.net\nmax_age: 604800\n";
        let policy = parse_mta_sts_policy(body).unwrap();
        assert_eq!(policy.mode, MtaStsMode::Enforce);
        assert_eq!(policy.mx, vec!["*.example.com", "mail.example.net"]);
        assert_eq!(policy.max_age, 604_800);
    }

    #[test]
    fn parse_rejects_wrong_version() {
        let body = "version: STSv2\nmode: enforce\nmx: *.example.com\nmax_age: 86400\n";
        assert!(parse_mta_sts_policy(body).is_none());
    }

    #[test]
    fn mx_allowed() {
        let policy = MtaStsPolicy {
            mode: MtaStsMode::Enforce,
            mx: vec!["*.example.com".to_string()],
            max_age: 86400,
        };
        assert!(mx_allowed_by_policy("mail.example.com", &policy));
        assert!(!mx_allowed_by_policy("mail.other.com", &policy));
    }
}
