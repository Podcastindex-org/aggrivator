# Web Bot Auth Request Signing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sign outbound feed requests per Cloudflare Web Bot Auth (RFC 9421 / Ed25519) so Cloudflare-protected publishers can verify the poller independent of source IP, with key tooling and JWKS emission, fully opt-in.

**Architecture:** A self-contained `signing` module (loaded via a new thin `lib.rs` so examples can reuse it) produces three signature headers for a given URL + key. The poller builds one shared signer from env config at startup and attaches headers per request. A keygen example generates the Ed25519 key and prints the JWKS directory.

**Tech Stack:** Rust, `ed25519-dalek` v2 (pkcs8/pem), `sha2`, `base64`, `serde_json`, `reqwest`.

Spec: `docs/superpowers/specs/2026-06-07-web-bot-auth-signing-design.md`

---

### Task 1: Dependencies, library target, and module scaffold

**Files:**
- Modify: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/signing.rs`

- [ ] **Step 1: Add dependencies to `Cargo.toml`**

In the `[dependencies]` section, after the `httpdate` line, add:

```toml
ed25519-dalek = { version = "2", features = ["pkcs8", "pem", "rand_core"] }
sha2 = "0.10"
base64 = "0.22"
serde_json = "1"
```

Then add a new section at the end of the file:

```toml
[dev-dependencies]
rand_core = { version = "0.6", features = ["getrandom"] }
```

- [ ] **Step 2: Create `src/lib.rs` exposing the module**

```rust
pub mod signing;
```

- [ ] **Step 3: Create `src/signing.rs` scaffold**

```rust
//! Web Bot Auth request signing (RFC 9421 HTTP Message Signatures, Ed25519).
//! See docs/superpowers/specs/2026-06-07-web-bot-auth-signing-design.md

use std::error::Error;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ed25519_dalek::pkcs8::DecodePrivateKey;
use ed25519_dalek::{Signer, SigningKey};
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::Url;
use sha2::{Digest, Sha256};

/// Signs outbound HTTP requests for Cloudflare Web Bot Auth.
pub struct WebBotAuthSigner {
    signing_key: SigningKey,
    keyid: String,
    signature_agent: String,
    ttl_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
}
```

- [ ] **Step 4: Build to verify it compiles**

Run: `cargo build`
Expected: PASS (warnings about unused items are fine).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/signing.rs
git commit -m "$(printf 'Add signing module scaffold and crypto deps\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 2: `compute_keyid` — RFC 7638 JWK thumbprint

**Files:**
- Modify: `src/signing.rs`

- [ ] **Step 1: Write the failing test**

Add inside `mod tests`:

```rust
    // RFC 8037 Appendix A.3 test vector: the JWK thumbprint of the Ed25519
    // public key with x="11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo".
    #[test]
    fn keyid_matches_rfc8037_thumbprint() {
        let pk_bytes = URL_SAFE_NO_PAD
            .decode("11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo")
            .unwrap();
        let mut pk = [0u8; 32];
        pk.copy_from_slice(&pk_bytes);
        assert_eq!(
            compute_keyid(&pk),
            "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib keyid_matches_rfc8037_thumbprint`
Expected: FAIL to compile — `cannot find function compute_keyid`.

- [ ] **Step 3: Write minimal implementation**

Add to `src/signing.rs` (module level, above `mod tests`):

```rust
/// RFC 7638 JWK thumbprint of an Ed25519 public key, base64url-no-pad.
fn compute_keyid(public_key: &[u8; 32]) -> String {
    let x = URL_SAFE_NO_PAD.encode(public_key);
    // Members in lexicographic order, no whitespace, per RFC 7638.
    let canonical = format!(r#"{{"crv":"Ed25519","kty":"OKP","x":"{}"}}"#, x);
    let digest = Sha256::digest(canonical.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib keyid_matches_rfc8037_thumbprint`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/signing.rs
git commit -m "$(printf 'Add RFC 7638 JWK thumbprint keyid computation\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 3: `authority_component` — the signed `@authority` value

**Files:**
- Modify: `src/signing.rs`

- [ ] **Step 1: Write the failing test**

Add inside `mod tests`:

```rust
    #[test]
    fn authority_lowercases_host_and_omits_default_port() {
        let url = Url::parse("https://Example.COM/feed/podcast/").unwrap();
        assert_eq!(authority_component(&url), "example.com");
    }

    #[test]
    fn authority_includes_non_default_port() {
        let url = Url::parse("https://example.com:8443/feed").unwrap();
        assert_eq!(authority_component(&url), "example.com:8443");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib authority`
Expected: FAIL to compile — `cannot find function authority_component`.

- [ ] **Step 3: Write minimal implementation**

Add to `src/signing.rs` (module level):

```rust
/// The RFC 9421 `@authority` derived component: lowercase host, plus the port
/// only when it is non-default for the scheme. `Url::port()` already returns
/// `None` for a scheme's default port (e.g. 443 for https).
fn authority_component(url: &Url) -> String {
    let host = url.host_str().unwrap_or("").to_lowercase();
    match url.port() {
        Some(port) => format!("{}:{}", host, port),
        None => host,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib authority`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add src/signing.rs
git commit -m "$(printf 'Add @authority component derivation\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 4: Signature params and signature base (exact strings)

**Files:**
- Modify: `src/signing.rs`

- [ ] **Step 1: Write the failing test**

Add inside `mod tests`:

```rust
    const KID: &str = "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k";

    #[test]
    fn signature_params_exact() {
        let params = signature_params(KID, 1735689600, 1735689900);
        assert_eq!(
            params,
            "(\"@authority\" \"signature-agent\");created=1735689600;expires=1735689900;keyid=\"kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k\";alg=\"ed25519\";tag=\"web-bot-auth\""
        );
    }

    #[test]
    fn signature_base_exact() {
        let params = signature_params(KID, 1735689600, 1735689900);
        let base = build_signature_base("example.com", "\"https://podcastindex.org\"", &params);
        let expected = "\"@authority\": example.com\n\
\"signature-agent\": \"https://podcastindex.org\"\n\
\"@signature-params\": (\"@authority\" \"signature-agent\");created=1735689600;expires=1735689900;keyid=\"kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k\";alg=\"ed25519\";tag=\"web-bot-auth\"";
        assert_eq!(base, expected);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib signature_`
Expected: FAIL to compile — `cannot find function signature_params` / `build_signature_base`.

- [ ] **Step 3: Write minimal implementation**

Add to `src/signing.rs` (module level):

```rust
/// The RFC 9421 signature parameters string: the inner component list plus
/// parameters, in a fixed canonical order. This exact string appears both in
/// `Signature-Input` (after `sig1=`) and as the `@signature-params` value.
fn signature_params(keyid: &str, created: u64, expires: u64) -> String {
    format!(
        "(\"@authority\" \"signature-agent\");created={};expires={};keyid=\"{}\";alg=\"ed25519\";tag=\"web-bot-auth\"",
        created, expires, keyid
    )
}

/// The RFC 9421 signature base: one line per covered component, then the
/// `@signature-params` line. `sig_agent_quoted` is the structured-field string
/// including its surrounding double quotes.
fn build_signature_base(authority: &str, sig_agent_quoted: &str, params: &str) -> String {
    format!(
        "\"@authority\": {}\n\"signature-agent\": {}\n\"@signature-params\": {}",
        authority, sig_agent_quoted, params
    )
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib signature_`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add src/signing.rs
git commit -m "$(printf 'Add RFC 9421 signature params and base construction\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 5: `WebBotAuthSigner` — load, sign, jwks

**Files:**
- Modify: `src/signing.rs`

- [ ] **Step 1: Write the failing test**

Add inside `mod tests`:

```rust
    #[test]
    fn sign_emits_verifiable_signature() {
        use ed25519_dalek::{Signature, Verifier};
        use rand_core::OsRng;

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let keyid = compute_keyid(verifying_key.as_bytes());
        let signer = WebBotAuthSigner {
            signing_key,
            keyid,
            signature_agent: "https://podcastindex.org".to_string(),
            ttl_secs: 300,
        };

        let url = Url::parse("https://example.com/feed").unwrap();
        let headers = signer.sign(&url, 1735689600);

        let mut params = None;
        let mut sig_b64 = None;
        for (name, value) in &headers {
            let v = value.to_str().unwrap();
            match name.as_str() {
                "signature-input" => params = Some(v.strip_prefix("sig1=").unwrap().to_string()),
                "signature" => {
                    sig_b64 = Some(
                        v.strip_prefix("sig1=:")
                            .unwrap()
                            .strip_suffix(':')
                            .unwrap()
                            .to_string(),
                    )
                }
                "signature-agent" => assert_eq!(v, "\"https://podcastindex.org\""),
                other => panic!("unexpected header {}", other),
            }
        }

        let base = build_signature_base(
            "example.com",
            "\"https://podcastindex.org\"",
            &params.unwrap(),
        );
        let sig_bytes = STANDARD.decode(sig_b64.unwrap()).unwrap();
        let sig_arr: [u8; 64] = sig_bytes.try_into().unwrap();
        let sig = Signature::from_bytes(&sig_arr);
        assert!(verifying_key.verify(base.as_bytes(), &sig).is_ok());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib sign_emits_verifiable_signature`
Expected: FAIL to compile — no method `sign` on `WebBotAuthSigner`.

- [ ] **Step 3: Write minimal implementation**

Add to `src/signing.rs` an `impl WebBotAuthSigner` block (module level, above `mod tests`):

```rust
impl WebBotAuthSigner {
    /// Load a PKCS#8 Ed25519 private key from a PEM file. Validates that
    /// `signature_agent` is an `https://` URL and precomputes the keyid.
    pub fn from_pem_file(
        path: &str,
        signature_agent: String,
        ttl_secs: u64,
    ) -> Result<Self, Box<dyn Error>> {
        let pem = std::fs::read_to_string(path)?;
        let signing_key = SigningKey::from_pkcs8_pem(&pem)?;
        let parsed = Url::parse(&signature_agent)?;
        if parsed.scheme() != "https" {
            return Err(format!("signature agent must be https: {}", signature_agent).into());
        }
        let keyid = compute_keyid(signing_key.verifying_key().as_bytes());
        Ok(Self {
            signing_key,
            keyid,
            signature_agent,
            ttl_secs,
        })
    }

    pub fn keyid(&self) -> &str {
        &self.keyid
    }

    /// Produce the three Web Bot Auth request headers for one request to `url`
    /// signed at `now_unix` (Unix seconds).
    pub fn sign(&self, url: &Url, now_unix: u64) -> [(HeaderName, HeaderValue); 3] {
        let created = now_unix;
        let expires = now_unix + self.ttl_secs;
        let authority = authority_component(url);
        let sig_agent_quoted = format!("\"{}\"", self.signature_agent);
        let params = signature_params(&self.keyid, created, expires);
        let base = build_signature_base(&authority, &sig_agent_quoted, &params);

        let signature = self.signing_key.sign(base.as_bytes());
        let sig_value = format!("sig1=:{}:", STANDARD.encode(signature.to_bytes()));
        let input_value = format!("sig1={}", params);

        [
            (
                HeaderName::from_static("signature-agent"),
                HeaderValue::from_str(&sig_agent_quoted).expect("ascii signature-agent"),
            ),
            (
                HeaderName::from_static("signature-input"),
                HeaderValue::from_str(&input_value).expect("ascii signature-input"),
            ),
            (
                HeaderName::from_static("signature"),
                HeaderValue::from_str(&sig_value).expect("ascii signature"),
            ),
        ]
    }

    /// The JWKS directory contents to publish at the well-known path.
    pub fn jwks(&self) -> serde_json::Value {
        let x = URL_SAFE_NO_PAD.encode(self.signing_key.verifying_key().as_bytes());
        serde_json::json!({
            "keys": [
                { "kty": "OKP", "crv": "Ed25519", "x": x, "kid": self.keyid, "use": "sig" }
            ]
        })
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib`
Expected: PASS (all signing tests).

- [ ] **Step 5: Commit**

```bash
git add src/signing.rs
git commit -m "$(printf 'Add WebBotAuthSigner load, sign, and jwks\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 6: Keygen example

**Files:**
- Create: `examples/webbotauth_keygen.rs`

- [ ] **Step 1: Write the example**

```rust
// One-time admin tool: generate an Ed25519 signing key and print the JWKS
// directory to publish at /.well-known/http-message-signatures-directory.
//
// Usage:
//   cargo run --example webbotauth_keygen -- [output-key.pem]
//   AGGRIVATOR_SIGNATURE_AGENT=https://podcastindex.org cargo run --example webbotauth_keygen

use std::env;
use std::fs;
use std::path::Path;

use aggrivator::signing::WebBotAuthSigner;
use ed25519_dalek::pkcs8::{EncodePrivateKey, LineEnding};
use ed25519_dalek::SigningKey;
use rand_core::OsRng;

fn main() {
    let out = env::args().nth(1).unwrap_or_else(|| "signing-key.pem".to_string());
    let agent = env::var("AGGRIVATOR_SIGNATURE_AGENT")
        .unwrap_or_else(|_| "https://podcastindex.org".to_string());

    if Path::new(&out).exists() {
        eprintln!("Refusing to overwrite existing key file: {}", out);
        std::process::exit(1);
    }

    let signing_key = SigningKey::generate(&mut OsRng);
    let pem = signing_key
        .to_pkcs8_pem(LineEnding::LF)
        .expect("encode private key to PKCS#8 PEM");
    fs::write(&out, pem.as_bytes()).expect("write key file");
    println!("Wrote Ed25519 private key to {}", out);

    let signer = WebBotAuthSigner::from_pem_file(&out, agent, 300)
        .expect("load the key we just wrote");
    println!("keyid: {}", signer.keyid());
    println!("\nPublish this JWKS at /.well-known/http-message-signatures-directory:\n");
    println!(
        "{}",
        serde_json::to_string_pretty(&signer.jwks()).expect("serialize jwks")
    );
}
```

- [ ] **Step 2: Build the example**

Run: `cargo build --example webbotauth_keygen`
Expected: PASS.

- [ ] **Step 3: Run it and verify output**

Run: `cargo run --quiet --example webbotauth_keygen -- /tmp/test-key.pem`
Expected: prints `Wrote Ed25519 private key to /tmp/test-key.pem`, a `keyid:` line, and a pretty JWKS with `"kty": "OKP"`, `"crv": "Ed25519"`, an `x` value, and a `kid` equal to the keyid.

- [ ] **Step 4: Clean up the throwaway key**

Run: `rm -f /tmp/test-key.pem`

- [ ] **Step 5: Commit**

```bash
git add examples/webbotauth_keygen.rs
git commit -m "$(printf 'Add Web Bot Auth keygen example\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 7: Integrate signing into the poller

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add imports**

At the top of `src/main.rs`, after the existing `use` lines (after `use httpdate;`), add:

```rust
use std::sync::Arc;
use aggrivator::signing::WebBotAuthSigner;
```

- [ ] **Step 2: Add the `build_signer` helper**

Add this function to `src/main.rs` (e.g. just above `fn main`):

```rust
//##: Build the optional Web Bot Auth signer from env config. Signing is opt-in:
//##: if no key is configured or it fails to load, we run unsigned (as before).
fn build_signer() -> Option<Arc<WebBotAuthSigner>> {
    let key_path = match std::env::var("AGGRIVATOR_SIGNING_KEY") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            println!("Web Bot Auth signing disabled (AGGRIVATOR_SIGNING_KEY not set)");
            return None;
        }
    };
    let agent = std::env::var("AGGRIVATOR_SIGNATURE_AGENT")
        .unwrap_or_else(|_| "https://podcastindex.org".to_string());
    let ttl: u64 = std::env::var("AGGRIVATOR_SIGNATURE_TTL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    match WebBotAuthSigner::from_pem_file(&key_path, agent, ttl) {
        Ok(signer) => {
            println!("Web Bot Auth signing enabled (keyid={})", signer.keyid());
            Some(Arc::new(signer))
        }
        Err(e) => {
            eprintln!(
                "Web Bot Auth signing disabled: failed to load key [{}]: {}",
                key_path, e
            );
            None
        }
    }
}
```

- [ ] **Step 3: Build the signer in `main` and pass it to `fetch_feeds`**

In `fn main`, replace this block:

```rust
    let podcasts = get_feeds_from_sql(sqlite_file);
    match podcasts {
        Ok(podcasts) => {
            match fetch_feeds(podcasts).await {
```

with:

```rust
    let signer = build_signer();
    let podcasts = get_feeds_from_sql(sqlite_file);
    match podcasts {
        Ok(podcasts) => {
            match fetch_feeds(podcasts, signer).await {
```

- [ ] **Step 4: Thread the signer through `fetch_feeds`**

Change the `fetch_feeds` signature from:

```rust
async fn fetch_feeds(podcasts: Vec<Podcast>) -> Result<(), Box<dyn std::error::Error>> {
```

to:

```rust
async fn fetch_feeds(
    podcasts: Vec<Podcast>,
    signer: Option<Arc<WebBotAuthSigner>>,
) -> Result<(), Box<dyn std::error::Error>> {
```

Then inside the `.map(|podcast| {` closure, add a per-task clone as the first line of the closure body (before `async move`):

```rust
        podcasts.into_iter().map(|podcast| {
            let signer = signer.clone();
            async move {
                match check_feed_is_updated(&podcast.url, &podcast.etag.as_str(), podcast.last_modified, podcast.id, signer.as_deref()).await {
```

(Only the `let signer = signer.clone();` line and the extra `signer.as_deref()` argument are new.)

- [ ] **Step 5: Accept the signer in `check_feed_is_updated` and sign the request**

Change the signature from:

```rust
async fn check_feed_is_updated(url: &str, etag: &str, last_modified: u64, feed_id: u64) -> Result<bool, Box<dyn Error>> {
```

to:

```rust
async fn check_feed_is_updated(url: &str, etag: &str, last_modified: u64, feed_id: u64, signer: Option<&WebBotAuthSigner>) -> Result<bool, Box<dyn Error>> {
```

Then replace this line:

```rust
    let response = client.get(url).send().await;
```

with:

```rust
    //Attach Web Bot Auth signature headers per-request (the signature binds the
    //target @authority and a created/expires window, so it cannot be a client
    //default header). On any error we simply send the request unsigned.
    let mut req = client.get(url);
    if let Some(signer) = signer {
        if let Ok(parsed) = reqwest::Url::parse(url) {
            if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
                for (name, value) in signer.sign(&parsed, now.as_secs()) {
                    req = req.header(name, value);
                }
            }
        }
    }
    let response = req.send().await;
```

- [ ] **Step 6: Build and run the unit tests**

Run: `cargo build && cargo test --lib`
Expected: PASS (build clean, all signing unit tests pass).

- [ ] **Step 7: Manually verify the opt-in (unsigned) path**

Run: `cargo run --quiet 2>&1 | head -5`
Expected: output includes `Web Bot Auth signing disabled (AGGRIVATOR_SIGNING_KEY not set)` and the poller proceeds exactly as before (it will read `feed_poller_queue.db` and start polling).

- [ ] **Step 8: Manually verify the enabled path loads a key**

Run:
```bash
cargo run --quiet --example webbotauth_keygen -- /tmp/sign.pem >/dev/null
AGGRIVATOR_SIGNING_KEY=/tmp/sign.pem cargo run --quiet 2>&1 | head -3
rm -f /tmp/sign.pem
```
Expected: output includes `Web Bot Auth signing enabled (keyid=...)`.

- [ ] **Step 9: Commit**

```bash
git add src/main.rs
git commit -m "$(printf 'Sign outbound feed requests with Web Bot Auth when configured\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 8: Live-verifier mode in the probe example

**Files:**
- Modify: `examples/probe.rs`

- [ ] **Step 1: Add the import**

At the top of `examples/probe.rs`, after the existing `use` lines, add:

```rust
use std::time::{SystemTime, UNIX_EPOCH};
use aggrivator::signing::WebBotAuthSigner;
```

- [ ] **Step 2: Sign the probe request when `SIGN_KEY` is set**

In `examples/probe.rs`, find where the request is sent:

```rust
    match client.get(&url).send().await {
```

Replace it with a builder that conditionally attaches signature headers:

```rust
    let mut req = client.get(&url);
    if let Ok(key_path) = env::var("SIGN_KEY") {
        let agent = env::var("AGGRIVATOR_SIGNATURE_AGENT")
            .unwrap_or_else(|_| "https://podcastindex.org".to_string());
        let signer = WebBotAuthSigner::from_pem_file(&key_path, agent, 300)
            .expect("load SIGN_KEY");
        println!("  + Web Bot Auth signing (keyid={})", signer.keyid());
        if let Ok(parsed) = reqwest::Url::parse(&url) {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            for (name, value) in signer.sign(&parsed, now) {
                req = req.header(name, value);
            }
        }
    }
    match req.send().await {
```

- [ ] **Step 3: Build the example**

Run: `cargo build --example probe`
Expected: PASS.

- [ ] **Step 4: Verify against Cloudflare's live verifier**

Run:
```bash
cargo run --quiet --example webbotauth_keygen -- /tmp/sign.pem >/dev/null
SIGN_KEY=/tmp/sign.pem cargo run --quiet --example probe -- https://http-message-signatures-example.research.cloudflare.com/
rm -f /tmp/sign.pem
```
Expected: the probe prints `+ Web Bot Auth signing (keyid=...)`, sends the three signature headers, and the endpoint's response reflects that a signature was received and parsed. Note: this endpoint verifies signature *structure/parsing* against the supplied `keyid`; full "verified bot" status additionally requires the directory to be published at `https://podcastindex.org/.well-known/http-message-signatures-directory` and registered with Cloudflare. Record the observed status and body. If the response indicates a malformed signature, the signature base in Task 4 is the first place to check.

- [ ] **Step 5: Clean up**

Run: `rm -f /tmp/sign.pem`

- [ ] **Step 6: Commit**

```bash
git add examples/probe.rs
git commit -m "$(printf 'Add Web Bot Auth signing mode to probe example\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 9: Packaging — version, README, BOT.md

**Files:**
- Modify: `Cargo.toml`
- Modify: `README.md`
- Modify: `BOT.md`

- [ ] **Step 1: Bump the version**

In `Cargo.toml`, change:

```toml
version = "0.1.9"
```

to:

```toml
version = "0.1.10"
```

- [ ] **Step 2: Add the README worklog entry**

In `README.md`, immediately under `## Worklog`, add a new entry above the `v0.1.9` entry:

```markdown
v0.1.10
 - Add optional Web Bot Auth request signing (RFC 9421 / Ed25519) so Cloudflare-protected feeds can verify the poller. Opt-in via AGGRIVATOR_SIGNING_KEY.
```

- [ ] **Step 3: Add a Web Bot Auth section to `BOT.md`**

In `BOT.md`, add this section immediately before the `## Contact / abuse` section:

```markdown
## Web Bot Auth (cryptographic verification)

Aggrivator can cryptographically sign its requests using [Web Bot Auth](https://developers.cloudflare.com/bots/reference/bot-verification/web-bot-auth/)
(RFC 9421 HTTP Message Signatures, Ed25519). Signed requests carry `Signature`,
`Signature-Input`, and `Signature-Agent` headers, with `tag="web-bot-auth"` and a
`keyid` that is the RFC 7638 JWK thumbprint of our published key.

Our public signing key is published as a JSON Web Key Set at:

```
https://podcastindex.org/.well-known/http-message-signatures-directory
```

To verify a request: confirm the `User-Agent` matches `Aggrivator (PodcastIndex.org)/v*`,
then verify the signature against the key whose thumbprint equals the request's `keyid`.
```

- [ ] **Step 4: Build to confirm the version bump is consistent**

Run: `cargo build && grep -A1 '^name = "aggrivator"' Cargo.lock`
Expected: build PASS and `Cargo.lock` shows `version = "0.1.10"`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock README.md BOT.md
git commit -m "$(printf 'Release v0.1.10: Web Bot Auth signing\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

## Notes for the implementer

- **Run all unit tests at the end:** `cargo test --lib` should pass with the `keyid`, `authority`, `signature_params`, `signature_base`, and `sign_emits_verifiable_signature` tests green.
- **The authoritative correctness check** is Task 8, Step 4 against Cloudflare's live verifier. If it reports an invalid signature, the most likely culprit is the signature base string in Task 4 — recheck byte-for-byte (component ordering, the quotes around the `signature-agent` value, the `@signature-params` value matching `Signature-Input` exactly).
- **Do not** put signature headers in the client `default_headers`; they must be per-request (Task 7, Step 5).
- **Cross-host redirects** are knowingly left unsigned beyond the first hop (see spec Limitations); do not attempt to fix that here.
