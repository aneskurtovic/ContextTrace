//! LiteLLM's public catalog, pinned to upstream Git revisions for history.
//!
//! No prompts, paths, IDs or model names leave the machine. Requests retrieve
//! the shared catalog and its revision dates. Git dates describe when LiteLLM
//! recorded a price, not a vendor's contractual effective date.

use chrono::{DateTime, Utc};
use ct_domain::pricing::{ModelRate, PriceQuote, PricingProvider};
use ct_domain::TokenUsage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const FILE: &str = "model_prices_and_context_window.json";
const COMMITS: &str = "https://api.github.com/repos/BerriAI/litellm/commits";
const RAW: &str = "https://raw.githubusercontent.com/BerriAI/litellm";
const MAX_BYTES: u64 = 16 * 1024 * 1024;
const REFRESH_SECONDS: i64 = 3600;

trait Http: Send + Sync {
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
}

#[derive(Default)]
struct PublicHttp(OnceLock<reqwest::blocking::Client>);

impl Http for PublicHttp {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        // Build lazily on the synchronous worker, not during async app setup.
        if self.0.get().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let client = reqwest::blocking::Client::builder()
                .user_agent(concat!("ContextTrace/", env!("CARGO_PKG_VERSION")))
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(12))
                .build()
                .map_err(|e| format!("pricing HTTP client: {e}"))?;
            let _ = self.0.set(client);
        }
        let response = self
            .0
            .get()
            .expect("HTTP client initialized")
            .get(url)
            .send()
            .map_err(|e| format!("pricing request failed: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "pricing service returned HTTP {status}; GitHub's public API may be rate limited"
            ));
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("pricing response: {e}"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("pricing response exceeded the size limit".into());
        }
        Ok(bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Revision {
    sha: String,
    recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Index {
    fetched_at: DateTime<Utc>,
    revisions: Vec<Revision>,
}

#[derive(Default)]
struct Cache {
    indices: BTreeMap<String, Index>,
    snapshots: BTreeMap<String, Arc<Value>>,
    failed: Option<(Instant, String)>,
}

pub struct LiteLlmPricing {
    directory: PathBuf,
    http: Box<dyn Http>,
    cache: Mutex<Cache>,
}

impl LiteLlmPricing {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            http: Box::<PublicHttp>::default(),
            cache: Mutex::default(),
        }
    }

    fn request(&self, cache: &mut Cache, url: &str) -> Result<Vec<u8>, String> {
        if let Some((when, error)) = &cache.failed {
            if when.elapsed() < Duration::from_secs(60) {
                return Err(error.clone());
            }
        }
        match self.http.get(url) {
            Ok(bytes) => {
                cache.failed = None;
                Ok(bytes)
            }
            Err(error) => {
                cache.failed = Some((Instant::now(), error.clone()));
                Err(error)
            }
        }
    }

    fn index(
        &self,
        cache: &mut Cache,
        at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<(Index, bool), String> {
        if at.is_some_and(|at| at > now) {
            return Err("the recorded request timestamp is in the future".into());
        }
        let key = at
            .map(|at| at.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "current".into());
        let path = self.directory.join(format!("index-{key}.json"));
        let saved = cache
            .indices
            .get(&key)
            .cloned()
            .or_else(|| read_json::<Index>(&path).filter(valid_index));
        let end = at.map(|at| at.date_naive().and_hms_opt(23, 59, 59).unwrap().and_utc());
        let fresh = saved.as_ref().is_some_and(|index| {
            index.fetched_at <= now
                && (end.is_some_and(|end| index.fetched_at > end)
                    || now.signed_duration_since(index.fetched_at).num_seconds() < REFRESH_SECONDS)
        });
        if fresh {
            let index = saved.unwrap();
            cache.indices.insert(key, index.clone());
            return Ok((index, false));
        }
        // Fetch the day's changes AND a preceding revision in one request.
        // A full page without a preceding revision is rejected, not truncated.
        let url = match end {
            Some(end) => format!(
                "{COMMITS}?path={FILE}&per_page=100&until={}",
                end.min(now)
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            ),
            None => format!("{COMMITS}?path={FILE}&per_page=1"),
        };
        let fetched = self
            .request(cache, &url)
            .and_then(|bytes| parse_index(&bytes, now, at));
        match fetched {
            Ok(index) => {
                write_json(&path, &index);
                if cache.indices.len() >= 64 {
                    cache.indices.clear();
                }
                cache.indices.insert(key, index.clone());
                Ok((index, false))
            }
            Err(error) => match saved {
                Some(index) => Ok((index, true)),
                None => Err(error),
            },
        }
    }

    fn snapshot(&self, cache: &mut Cache, revision: &Revision) -> Result<Arc<Value>, String> {
        if let Some(value) = cache.snapshots.get(&revision.sha) {
            return Ok(Arc::clone(value));
        }
        let path = self.directory.join(format!("{}.json", revision.sha));
        let value = match read_json::<Value>(&path).filter(valid_snapshot) {
            Some(value) => value,
            None => {
                let bytes = self.request(cache, &format!("{RAW}/{}/{FILE}", revision.sha))?;
                let value: Value = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("invalid upstream pricing catalog: {e}"))?;
                if !valid_snapshot(&value) {
                    return Err("upstream pricing catalog has no usable token prices".into());
                }
                write_json(&path, &value);
                value
            }
        };
        let value = Arc::new(value);
        if cache.snapshots.len() >= 8 {
            cache.snapshots.clear();
        }
        cache
            .snapshots
            .insert(revision.sha.clone(), Arc::clone(&value));
        Ok(value)
    }
}

impl PricingProvider for LiteLlmPricing {
    fn quote(
        &self,
        model: &str,
        at: Option<DateTime<Utc>>,
        usage: TokenUsage,
    ) -> Result<PriceQuote, String> {
        let now: DateTime<Utc> = SystemTime::now().into();
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| "pricing cache lock is unavailable")?;
        let (index, stale) = self.index(&mut cache, at, now)?;
        let revision = index
            .revisions
            .iter()
            .filter(|revision| revision.recorded_at <= at.unwrap_or(now))
            .max_by_key(|revision| revision.recorded_at)
            .ok_or("no upstream pricing revision was recorded before this request")?;
        let value = self.snapshot(&mut cache, revision)?;
        let rate = resolve_rate(&value, model, usage)?;
        let mut warning = if at.is_some() {
            "Historical list-price estimate using the LiteLLM revision recorded at or before each request. Git dates are observation dates, not guaranteed vendor effective dates."
        } else {
            "Current list-price estimate; the request has no timestamp, so historical pricing cannot be established."
        }.to_string();
        warning.push_str(" Standard text-token rates; subscriptions, negotiated discounts, service tiers, tools and media fees are not included. Cache writes use the catalog's default duration.");
        if stale {
            warning.push_str(&format!(
                " Offline or refresh failed: cached catalog index from {} is stale.",
                index.fetched_at
            ));
        }
        Ok(PriceQuote {
            rate,
            version: format!("LiteLLM {} ({})", &revision.sha[..12], revision.recorded_at),
            source: format!("{RAW}/{}/{FILE}", revision.sha),
            warning,
        })
    }
}

fn valid_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|c| c.is_ascii_hexdigit())
}

fn valid_index(index: &Index) -> bool {
    !index.revisions.is_empty()
        && index
            .revisions
            .iter()
            .all(|revision| valid_sha(&revision.sha))
}

fn valid_snapshot(value: &Value) -> bool {
    value.as_object().is_some_and(|models| {
        models.iter().any(|(name, model)| {
            name != "sample_spec"
                && model["input_cost_per_token"].is_number()
                && model["output_cost_per_token"].is_number()
        })
    })
}

fn parse_index(
    bytes: &[u8],
    now: DateTime<Utc>,
    at: Option<DateTime<Utc>>,
) -> Result<Index, String> {
    let rows: Vec<Value> =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid pricing history: {e}"))?;
    let mut revisions = Vec::new();
    for row in &rows {
        let sha = row["sha"]
            .as_str()
            .filter(|sha| valid_sha(sha))
            .ok_or("invalid pricing revision ID")?;
        let recorded_at = row["commit"]["committer"]["date"]
            .as_str()
            .and_then(|date| DateTime::parse_from_rfc3339(date).ok())
            .ok_or("invalid pricing revision date")?
            .with_timezone(&Utc);
        if recorded_at <= now {
            revisions.push(Revision {
                sha: sha.into(),
                recorded_at,
            });
        }
    }
    if let Some(at) = at {
        let start = at.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
        if rows.len() >= 100
            && !revisions
                .iter()
                .any(|revision| revision.recorded_at < start)
        {
            return Err("upstream pricing history page was truncated; no historical rate can be established".into());
        }
    }
    let index = Index {
        fetched_at: now,
        revisions,
    };
    if !valid_index(&index) {
        return Err("no pricing history is available for this date".into());
    }
    Ok(index)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    if fs::metadata(path).ok()?.len() > MAX_BYTES {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn write_json(path: &Path, value: &impl Serialize) {
    // Derived data: failures to persist never stop inspection. A partial write
    // must not replace a known-good snapshot or index.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let Some(parent) = path.parent() else {
        return;
    };
    let Ok(bytes) = serde_json::to_vec(value) else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let temp = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    if fs::write(&temp, bytes).is_ok() {
        let _ = fs::rename(&temp, path);
    }
    let _ = fs::remove_file(temp);
}

fn resolve_rate(catalog: &Value, name: &str, usage: TokenUsage) -> Result<ModelRate, String> {
    // Exact IDs only. In particular, gpt-4.1-mini must never inherit gpt-4.1.
    // Provider-qualified IDs retain their provider; no suffix/provider guessing.
    let name = name.trim().to_ascii_lowercase();
    let model = catalog
        .get(&name)
        .ok_or("the upstream catalog has no exact match for this model")?;
    if name == "sample_spec" {
        return Err("catalog schema is not a model".into());
    }
    if model["mode"]
        .as_str()
        .is_some_and(|mode| !matches!(mode, "chat" | "completion"))
    {
        return Err("this model does not use supported text-token pricing".into());
    }
    let prompt = u64::from(usage.input.unwrap_or(0))
        + u64::from(usage.cache_read.unwrap_or(0))
        + u64::from(usage.cache_creation.unwrap_or(0));
    let price = |field: &str, tokens: Option<u32>| -> Result<u64, String> {
        let mut selected = model.get(field);
        let mut threshold = 0;
        if let Some(fields) = model.as_object() {
            for (key, value) in fields {
                if let Some(size) = key
                    .strip_prefix(&format!("{field}_above_"))
                    .and_then(|s| s.strip_suffix("_tokens"))
                {
                    let parsed = size
                        .strip_suffix('k')
                        .and_then(|s| s.parse::<u64>().ok())
                        .and_then(|n| n.checked_mul(1000))
                        .or_else(|| size.parse::<u64>().ok())
                        .ok_or("unsupported context pricing threshold")?;
                    if prompt > parsed && parsed > threshold {
                        threshold = parsed;
                        selected = Some(value);
                    }
                }
            }
        }
        let Some(value) = selected else {
            return if tokens.unwrap_or(0) == 0 {
                Ok(0)
            } else {
                Err(format!(
                    "upstream catalog is missing {field} for recorded usage"
                ))
            };
        };
        let dollars_per_token = value
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .ok_or_else(|| format!("invalid upstream rate for {field}"))?;
        let micros_per_million = dollars_per_token * 1_000_000_000_000.0;
        if micros_per_million >= u64::MAX as f64 {
            return Err("upstream price exceeds supported range".into());
        }
        Ok(micros_per_million.round() as u64)
    };
    Ok(ModelRate {
        input_per_million: price("input_cost_per_token", usage.input)?,
        output_per_million: price("output_cost_per_token", usage.output)?,
        cache_read_per_million: price("cache_read_input_token_cost", usage.cache_read)?,
        cache_write_per_million: price("cache_creation_input_token_cost", usage.cache_creation)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn all_observed_public_model_ids_have_usable_catalog_rates() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/pricing/litellm-text-prices.json"))
                .unwrap();
        for name in [
            "gpt-5.4-mini",
            "gpt-5.6-luna",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-6-luna",
            "gpt-6-sol",
            "gpt-6-astra",
            "gpt-6.1-sol",
            "claude-opus-5",
            "claude-opus-5-5",
            "claude-haiku-4-5-20251001",
        ] {
            for input in [1000, 300_000] {
                let rate = resolve_rate(
                    &fixture["models"],
                    name,
                    TokenUsage {
                        input: Some(input),
                        output: Some(1000),
                        cache_read: Some(1000),
                        cache_creation: name.starts_with("claude-").then_some(1000),
                        ..Default::default()
                    },
                )
                .unwrap_or_else(|e| panic!("{name}: {e}"));
                assert!(rate.input_per_million > 0 && rate.output_per_million > 0);
                assert!(rate.cache_read_per_million > 0);
                if name.starts_with("claude-") {
                    assert!(rate.cache_write_per_million > 0);
                }
            }
        }
        for name in ["codex-auto-review", "<synthetic>"] {
            assert!(resolve_rate(&fixture["models"], name, TokenUsage::default()).is_err());
        }
    }

    #[test]
    fn exact_ids_units_cache_and_context_tiers() {
        let catalog = json!({"gpt-4.1": {"input_cost_per_token": 0.000002, "output_cost_per_token": 0.000008},
            "gpt-4.1-mini": {"input_cost_per_token": 0.0000004, "output_cost_per_token": 0.0000016},
            "claude-test": {"input_cost_per_token": 0.000003, "output_cost_per_token": 0.000015,
                "cache_read_input_token_cost": 0.0000003, "cache_creation_input_token_cost": 0.00000375,
                "input_cost_per_token_above_200k_tokens": 0.000006, "output_cost_per_token_above_200k_tokens": 0.0000225}});
        let rate = resolve_rate(
            &catalog,
            "gpt-4.1-mini",
            TokenUsage {
                input: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rate.input_per_million, 400_000);
        assert!(resolve_rate(&catalog, "gpt-4.1-mini-new", TokenUsage::default()).is_err());
        assert!(resolve_rate(&catalog, "openrouter/gpt-4.1", TokenUsage::default()).is_err());
        let rate = resolve_rate(
            &catalog,
            "claude-test",
            TokenUsage {
                input: Some(200_001),
                cache_read: Some(10),
                cache_creation: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rate.input_per_million, 6_000_000);
        assert_eq!(rate.output_per_million, 22_500_000);
        assert_eq!(rate.cache_read_per_million, 300_000);
        assert_eq!(rate.cache_write_per_million, 3_750_000);
    }

    #[test]
    fn absent_or_invalid_prices_are_not_free_usage() {
        for invalid in [json!(-1), json!("0.01"), Value::Null] {
            let catalog = json!({"test": {"input_cost_per_token": invalid}});
            assert!(resolve_rate(
                &catalog,
                "test",
                TokenUsage {
                    input: Some(1),
                    ..Default::default()
                }
            )
            .is_err());
        }
        let catalog = json!({"test": {"input_cost_per_token": 0, "output_cost_per_token": 0}});
        assert_eq!(
            resolve_rate(
                &catalog,
                "test",
                TokenUsage {
                    input: Some(1),
                    ..Default::default()
                }
            )
            .unwrap()
            .input_per_million,
            0
        );
        assert!(resolve_rate(
            &catalog,
            "test",
            TokenUsage {
                cache_read: Some(1),
                ..Default::default()
            }
        )
        .is_err());
    }

    type FakeResponse = (String, Result<Vec<u8>, String>);
    struct FakeHttp {
        responses: Mutex<Vec<FakeResponse>>,
    }
    impl Http for FakeHttp {
        fn get(&self, url: &str) -> Result<Vec<u8>, String> {
            let (expected, result) = self.responses.lock().unwrap().remove(0);
            assert!(url.contains(&expected), "{url} did not contain {expected}");
            result
        }
    }
    fn date(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn history_selects_prior_revision_survives_restart_and_never_backdates_current_prices() {
        let directory = std::env::temp_dir().join(format!("ct-pricing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let older = "a".repeat(40);
        let newer = "b".repeat(40);
        let history = json!([
            {"sha": newer, "commit": {"committer": {"date": "2020-01-02T12:00:00Z"}}},
            {"sha": older, "commit": {"committer": {"date": "2020-01-01T10:00:00Z"}}}
        ]);
        let catalog =
            json!({"test": {"input_cost_per_token": 0.000002, "output_cost_per_token": 0.000008}});
        let provider = LiteLlmPricing {
            directory: directory.clone(),
            cache: Mutex::default(),
            http: Box::new(FakeHttp {
                responses: Mutex::new(vec![
                    (
                        "until=2020-01-02T23:59:59Z".into(),
                        Ok(serde_json::to_vec(&history).unwrap()),
                    ),
                    (older.clone(), Ok(serde_json::to_vec(&catalog).unwrap())),
                    (newer.clone(), Ok(serde_json::to_vec(&catalog).unwrap())),
                ]),
            }),
        };
        let early = provider
            .quote(
                "test",
                Some(date("2020-01-02T11:00:00Z")),
                TokenUsage::default(),
            )
            .unwrap();
        assert!(early.source.contains(&older));
        let late = provider
            .quote(
                "test",
                Some(date("2020-01-02T13:00:00Z")),
                TokenUsage::default(),
            )
            .unwrap();
        assert!(late.source.contains(&newer));
        let offline = LiteLlmPricing {
            directory: directory.clone(),
            cache: Mutex::default(),
            http: Box::new(FakeHttp {
                responses: Mutex::new(vec![]),
            }),
        };
        assert_eq!(
            offline
                .quote(
                    "test",
                    Some(date("2020-01-02T11:00:00Z")),
                    TokenUsage::default()
                )
                .unwrap(),
            early
        );
        assert!(offline
            .quote(
                "new-model",
                Some(date("2020-01-02T11:00:00Z")),
                TokenUsage::default()
            )
            .is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn stale_current_index_remains_usable_and_failed_requests_are_throttled() {
        let directory =
            std::env::temp_dir().join(format!("ct-pricing-offline-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let revision = Revision {
            sha: "c".repeat(40),
            recorded_at: date("2020-01-01T00:00:00Z"),
        };
        write_json(
            &directory.join("index-current.json"),
            &Index {
                fetched_at: revision.recorded_at,
                revisions: vec![revision.clone()],
            },
        );
        write_json(
            &directory.join(format!("{}.json", revision.sha)),
            &json!({"test": {"input_cost_per_token": 0.000002, "output_cost_per_token": 0.000008}}),
        );
        let provider = LiteLlmPricing {
            directory: directory.clone(),
            cache: Mutex::default(),
            http: Box::new(FakeHttp {
                responses: Mutex::new(vec![("per_page=1".into(), Err("offline".into()))]),
            }),
        };
        assert!(provider
            .quote("test", None, TokenUsage::default())
            .unwrap()
            .warning
            .contains("stale"));
        assert!(provider
            .quote("test", None, TokenUsage::default())
            .unwrap()
            .warning
            .contains("stale"));
        fs::remove_dir_all(directory).unwrap();
    }
}
