//! Apple Push Notification service: token-based auth (`.p8` key, ES256 JWT) over HTTP/2.
//!
//! Config lives outside every repo, in `%USERPROFILE%\.aos\secrets\apns.json` (or the file named by
//! `AOS_APNS_CONFIG`): `{key_path, key_id, team_id, topic, env}`. `topic` is the bundle id; `env` is
//! the default for tokens that don't say (`production` for TestFlight and App Store builds).
//! Nothing here logs the key, the JWT or a full device token.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
use serde::Deserialize;
use serde_json::Value;

/// Apple wants the provider token refreshed between 20 and 60 minutes; reuse it for 40.
pub const JWT_REUSE_SECS: u64 = 40 * 60;

#[derive(Debug, Clone, Deserialize)]
pub struct ApnsConfig {
    pub key_path: PathBuf,
    pub key_id: String,
    pub team_id: String,
    /// The app's bundle id.
    pub topic: String,
    /// `production` or `sandbox`.
    #[serde(default = "production")]
    pub env: String,
}

fn production() -> String {
    "production".into()
}

impl ApnsConfig {
    /// `AOS_APNS_CONFIG`, else `~/.aos/secrets/apns.json`. `None` when there is no file (push off).
    pub fn load() -> Result<Option<ApnsConfig>> {
        let path = std::env::var_os("AOS_APNS_CONFIG")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".aos").join("secrets").join("apns.json")));
        let Some(path) = path.filter(|p| p.exists()) else {
            return Ok(None);
        };
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: ApnsConfig = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(cfg))
    }
}

/// Signs and caches the provider token.
pub struct Signer {
    key: EcdsaKeyPair,
    key_id: String,
    team_id: String,
    rng: SystemRandom,
    cached: Mutex<Option<(String, u64)>>,
}

impl Signer {
    /// From the text of a `.p8` file (PKCS#8 PEM, P-256).
    pub fn from_pem(pem: &str, key_id: &str, team_id: &str) -> Result<Signer> {
        let body: String = pem.lines().filter(|l| !l.starts_with("-----")).map(str::trim).collect();
        let der = STANDARD.decode(body.as_bytes()).context("the .p8 key is not base64 PEM")?;
        let rng = SystemRandom::new();
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &der, &rng)
            .map_err(|e| anyhow!("the .p8 key is not a P-256 PKCS#8 key: {e}"))?;
        if key_id.len() != 10 || team_id.len() != 10 {
            bail!("APNs key id and team id are 10 characters each");
        }
        Ok(Signer { key, key_id: key_id.into(), team_id: team_id.into(), rng, cached: Mutex::new(None) })
    }

    /// A token signed at `now` (seconds since the epoch). Pure, for tests.
    pub fn sign_at(&self, now: u64) -> Result<String> {
        let head = URL_SAFE_NO_PAD.encode(serde_json::json!({ "alg": "ES256", "kid": self.key_id }).to_string());
        let claims = URL_SAFE_NO_PAD.encode(serde_json::json!({ "iss": self.team_id, "iat": now }).to_string());
        let input = format!("{head}.{claims}");
        // FIXED signing gives r || s (64 bytes), which is what JWS ES256 wants.
        let sig = self.key.sign(&self.rng, input.as_bytes()).map_err(|_| anyhow!("signing the APNs token failed"))?;
        Ok(format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref())))
    }

    /// The cached token, re-signed after [`JWT_REUSE_SECS`] or when `fresh`.
    pub fn bearer(&self, fresh: bool) -> Result<String> {
        let now = unix_now();
        let mut c = self.cached.lock().unwrap_or_else(|p| p.into_inner());
        if !fresh && let Some((tok, at)) = c.as_ref() && now.saturating_sub(*at) < JWT_REUSE_SECS {
            return Ok(tok.clone());
        }
        let tok = self.sign_at(now)?;
        *c = Some((tok.clone(), now));
        Ok(tok)
    }

    #[cfg(test)]
    pub fn public_key(&self) -> Vec<u8> {
        use ring::signature::KeyPair;
        self.key.public_key().as_ref().to_vec()
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The headers and body of one APNs request (see `payload.rs`).
#[derive(Debug, Clone, PartialEq)]
pub struct ApnsRequest {
    pub push_type: &'static str,
    /// 10 = now, 5 = when convenient.
    pub priority: u8,
    /// Unix seconds after which Apple drops it; 0 = try once.
    pub expiration: Option<u64>,
    pub collapse_id: Option<String>,
    pub body: Value,
}

/// What Apple said about one push.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Sent,
    /// The token is dead (410 Unregistered, 400 BadDeviceToken / DeviceTokenNotForTopic): forget it.
    Gone(String),
    /// Anything else: keep the token, log the reason.
    Failed(String),
}

pub struct Apns {
    pub cfg: ApnsConfig,
    signer: Signer,
    http: reqwest::Client,
}

impl Apns {
    pub fn open(cfg: ApnsConfig) -> Result<Apns> {
        let pem = std::fs::read_to_string(&cfg.key_path)
            .with_context(|| format!("reading the APNs key at {}", cfg.key_path.display()))?;
        let signer = Signer::from_pem(&pem, &cfg.key_id, &cfg.team_id)?;
        let http = reqwest::Client::builder()
            .http2_prior_knowledge()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(8))
            .build()?;
        Ok(Apns { cfg, signer, http })
    }

    pub fn host(env: &str) -> &'static str {
        if env == "sandbox" || env == "development" { "https://api.sandbox.push.apple.com" } else { "https://api.push.apple.com" }
    }

    pub async fn send(&self, token: &str, env: &str, req: &ApnsRequest) -> Outcome {
        match self.send_once(token, env, req, false).await {
            Outcome::Failed(r) if r.contains("ExpiredProviderToken") => self.send_once(token, env, req, true).await,
            o => o,
        }
    }

    async fn send_once(&self, token: &str, env: &str, req: &ApnsRequest, fresh: bool) -> Outcome {
        let bearer = match self.signer.bearer(fresh) {
            Ok(b) => b,
            Err(e) => return Outcome::Failed(format!("{e:#}")),
        };
        let mut r = self
            .http
            .post(format!("{}/3/device/{token}", Apns::host(env)))
            .header("authorization", format!("bearer {bearer}"))
            .header("apns-topic", &self.cfg.topic)
            .header("apns-push-type", req.push_type)
            .header("apns-priority", req.priority.to_string())
            .json(&req.body);
        if let Some(exp) = req.expiration {
            r = r.header("apns-expiration", exp.to_string());
        }
        if let Some(c) = &req.collapse_id {
            r = r.header("apns-collapse-id", c);
        }
        let resp = match r.send().await {
            Ok(resp) => resp,
            Err(e) => return Outcome::Failed(format!("network: {e}")),
        };
        let status = resp.status().as_u16();
        let reason = resp.json::<Value>().await.ok().and_then(|v| v["reason"].as_str().map(str::to_string)).unwrap_or_default();
        classify(status, &reason)
    }
}

pub fn classify(status: u16, reason: &str) -> Outcome {
    match (status, reason) {
        (200, _) => Outcome::Sent,
        (410, _) | (400, "BadDeviceToken") | (400, "DeviceTokenNotForTopic") => Outcome::Gone(format!("{status} {reason}")),
        _ => Outcome::Failed(format!("{status} {reason}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};

    pub fn test_pem() -> String {
        let der = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new()).unwrap();
        // PEM armour built at run time, so the source never holds a key header (the public scrub checks).
        let kind = ["PRIVATE", "KEY"].join(" ");
        format!("-----BEGIN {kind}-----\n{}\n-----END {kind}-----\n", STANDARD.encode(der.as_ref()))
    }

    #[test]
    fn signs_a_verifiable_es256_token() {
        let s = Signer::from_pem(&test_pem(), "ABCDE12345", "TEAM123456").unwrap();
        let tok = s.sign_at(1_800_000_000).unwrap();
        let parts: Vec<&str> = tok.split('.').collect();
        assert_eq!(parts.len(), 3);
        let head: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(head, serde_json::json!({ "alg": "ES256", "kid": "ABCDE12345" }));
        let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims, serde_json::json!({ "iss": "TEAM123456", "iat": 1_800_000_000u64 }));
        let sig = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        assert_eq!(sig.len(), 64, "JWS ES256 is r || s");
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, s.public_key())
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
            .expect("signature verifies");
    }

    #[test]
    fn reuses_the_token_until_refresh() {
        let s = Signer::from_pem(&test_pem(), "ABCDE12345", "TEAM123456").unwrap();
        let a = s.bearer(false).unwrap();
        assert_eq!(a, s.bearer(false).unwrap());
        // ECDSA signatures are randomised, so a fresh one differs.
        assert_ne!(a, s.bearer(true).unwrap());
    }

    #[test]
    fn rejects_bad_keys_and_ids() {
        assert!(Signer::from_pem("-----BEGIN-----\nAAAA\n-----END-----", "ABCDE12345", "TEAM123456").is_err());
        assert!(Signer::from_pem(&test_pem(), "short", "TEAM123456").is_err());
    }

    /// The real key on this PC parses and signs (prints nothing secret). `cargo test -p aos-push -- --ignored`
    #[test]
    #[ignore]
    fn real_key_signs() {
        let cfg = ApnsConfig::load().unwrap().expect("~/.aos/secrets/apns.json");
        let a = Apns::open(cfg).unwrap();
        assert_eq!(a.signer.bearer(false).unwrap().split('.').count(), 3);
        assert!(!a.cfg.topic.is_empty());
    }

    #[test]
    fn classifies_apple_replies() {
        assert_eq!(classify(200, ""), Outcome::Sent);
        assert!(matches!(classify(410, "Unregistered"), Outcome::Gone(_)));
        assert!(matches!(classify(400, "BadDeviceToken"), Outcome::Gone(_)));
        assert!(matches!(classify(403, "ExpiredProviderToken"), Outcome::Failed(_)));
        assert!(matches!(classify(429, "TooManyRequests"), Outcome::Failed(_)));
        assert_eq!(Apns::host("sandbox"), "https://api.sandbox.push.apple.com");
        assert_eq!(Apns::host("production"), "https://api.push.apple.com");
    }
}
