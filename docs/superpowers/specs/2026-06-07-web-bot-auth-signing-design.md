# Web Bot Auth request signing — design

**Date:** 2026-06-07
**Branch:** `http-signing`
**Status:** Approved design, pending implementation plan

## Background

Aggrivator is the Podcast Index feed poller. Some publishers sit behind Cloudflare,
whose bot management can challenge or reject the poller's requests purely on source-IP
reputation (observed: `https://nakedbiblepodcast.com/feed/podcast/` returns a Cloudflare
JS challenge or an origin `415` to the poller's datacenter IPs, while the same request
from a clean IP returns the feed).

Cloudflare now supports verifying bots cryptographically via **Web Bot Auth**, built on
RFC 9421 (HTTP Message Signatures). A crawler signs each request with an Ed25519 key and
publishes the matching public key as a JWKS at a well-known path. Cloudflare (and any other
verifier) can then confirm the request genuinely comes from the registered bot, independent
of IP. This spec adds request signing to the poller.

Reference material:
- Cloudflare Web Bot Auth docs: https://developers.cloudflare.com/bots/reference/bot-verification/web-bot-auth/
- Cloudflare reference implementation: https://github.com/cloudflareresearch/web-bot-auth
- Live verifier (testing): https://http-message-signatures-example.research.cloudflare.com
- RFC 9421 (HTTP Message Signatures), RFC 7638 (JWK Thumbprint)

## Goals

- Sign outbound feed requests per Web Bot Auth so Cloudflare-protected publishers can verify the poller.
- Provide tooling to generate the Ed25519 key and emit the JWKS directory for hosting.
- Be **opt-in and non-breaking**: with no key configured, behaviour is byte-identical to today.

## Non-goals

- Hosting the key directory at `/.well-known/http-message-signatures-directory` (done by the
  podcastindex.org web server, not this poller).
- Signing the directory *response* (a serve-time concern; a static JWKS file cannot carry a
  time-bound directory signature).
- Re-signing across cross-host redirects (deferred; see Limitations).
- A `nonce` / replay-protection parameter (deferred; optional in v1).

## Scope decisions (from brainstorming)

| Decision | Choice |
|---|---|
| In-repo scope | Request signing **+** key tooling **+** emit JWKS file. Hosting elsewhere. |
| Key source | PKCS#8 Ed25519 PEM loaded from a file path. |
| No-key behaviour | Run **unsigned** (opt-in); load failure also falls back to unsigned. |
| `Signature-Agent` | **Included**, pointing to the published directory. Sign `("@authority" "signature-agent")`. |
| Implementation | Approach A — minimal hand-rolled signer (`ed25519-dalek` + `sha2` + `base64`), no RFC 9421 framework crate. |

## Architecture

### New module `src/signing.rs`

Self-contained Web Bot Auth surface, isolated from polling logic. Pure crypto/encoding;
unit-testable with no network.

```
struct WebBotAuthSigner {
    signing_key: ed25519_dalek::SigningKey,
    keyid: String,            // precomputed RFC 7638 JWK thumbprint
    signature_agent: String,  // directory URL, e.g. "https://podcastindex.org"
    ttl_secs: u64,            // expires window
}

impl WebBotAuthSigner {
    // Load PKCS#8 Ed25519 PEM, derive public key, compute keyid once.
    fn from_pem_file(path: &str, signature_agent: String, ttl_secs: u64) -> Result<Self, Box<dyn Error>>;

    // Produce the three request headers for one request to `url` at `now_unix`.
    fn sign(&self, url: &reqwest::Url, now_unix: u64) -> [(header::HeaderName, header::HeaderValue); 3];

    fn jwks(&self) -> serde_json::Value;  // directory contents
    fn keyid(&self) -> &str;
}
```

`now_unix` is a parameter (not read internally) so the signature base is deterministic and
unit-testable; the poller passes `SystemTime::now()`.

### New `examples/webbotauth_keygen.rs`

One-time admin tool reusing `signing.rs`: generates an Ed25519 PKCS#8 PEM, writes it to disk,
and prints the `keyid` and the JWKS JSON to paste into the published directory. Kept as an
example so the poller binary stays single-purpose.

### Touch-point in `src/main.rs`

- `main()` builds `Option<Arc<WebBotAuthSigner>>` once from env config, validating the key at
  startup, then threads it through `fetch_feeds` into `check_feed_is_updated`. `Arc` is shared
  read-only across the concurrent `buffer_unordered` tasks (`SigningKey` is `Send + Sync`).
- In `check_feed_is_updated`, signature headers are attached **per-request** on the
  `RequestBuilder` (not the client `default_headers`), because the signature binds `@authority`
  (per-feed) and `created/expires` (per-request):

```rust
let mut req = client.get(url);
if let Some(signer) = signer.as_deref() {
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

UA, `Accept`, and the conditional headers remain in `default_headers` unchanged.

## Crypto

### Key loading
`ed25519-dalek` v2 (`pkcs8`, `pem` features) parses a PKCS#8 PEM
(`openssl genpkey -algorithm ed25519 -out signing-key.pem`) into a `SigningKey`; derive the
32-byte `VerifyingKey`.

### keyid — RFC 7638 JWK thumbprint (computed once at load)
1. Canonical JWK, members lexicographically ordered, no whitespace:
   `{"crv":"Ed25519","kty":"OKP","x":"<B>"}`, where `<B>` = base64url-no-pad of the 32-byte public key.
2. SHA-256 of that exact UTF-8 string.
3. `keyid` = base64url-no-pad of the 32-byte digest.

### JWKS directory (emitted by keygen, hosted elsewhere)
```json
{
  "keys": [
    { "kty": "OKP", "crv": "Ed25519", "x": "<B>", "kid": "<keyid>", "use": "sig" }
  ]
}
```
Served with `Content-Type: application/http-message-signatures-directory+json`.
`kty`/`crv`/`x` are mandatory; `kid` (= the thumbprint) lets verifiers match `keyid`. The JWK
`alg` member is intentionally omitted — the registered JWA value for Ed25519 keys is `EdDSA` and
the member is optional, so we avoid publishing an ambiguous value. This is independent of the
RFC 9421 signature parameter `alg="ed25519"` used on the wire, which Cloudflare expects.

## Signature construction (RFC 9421)

- Covered components (fixed): `("@authority" "signature-agent")`
- Label: `sig1`
- Algorithm: Ed25519

### Signature parameters (canonical order, emitted identically in base and header)
```
created=<now>;expires=<now+TTL>;keyid="<keyid>";alg="ed25519";tag="web-bot-auth"
```
- `TTL` default 300s (covers the 30s request timeout + clock skew).
- No `nonce` in v1.

### Signature base (exact UTF-8 string that is signed)
For a request to `https://nakedbiblepodcast.com/feed/podcast/`:
```
"@authority": nakedbiblepodcast.com
"signature-agent": "https://podcastindex.org"
"@signature-params": ("@authority" "signature-agent");created=<now>;expires=<now+300>;keyid="<keyid>";alg="ed25519";tag="web-bot-auth"
```
- `@authority` = lowercase host, plus port only if non-default (omitted for 443).
- `signature-agent` value = the structured-field string **including** its surrounding quotes.
- `@signature-params` value = byte-identical to what follows `sig1=` in `Signature-Input`.

### Emitted headers
```
Signature-Agent: "https://podcastindex.org"
Signature-Input: sig1=("@authority" "signature-agent");created=<now>;expires=<now+300>;keyid="<keyid>";alg="ed25519";tag="web-bot-auth"
Signature: sig1=:<base64-standard-with-padding(signature)>:
```
`Signature` is `sf-binary` (base64 standard, padded, wrapped in colons); `Signature-Input` /
`Signature-Agent` are structured-field serializations.

## Configuration (env vars; opt-in)

| Var | Meaning | Default |
|---|---|---|
| `AGGRIVATOR_SIGNING_KEY` | Path to PKCS#8 Ed25519 PEM. **Unset → signing disabled** (runs as today). | unset |
| `AGGRIVATOR_SIGNATURE_AGENT` | Directory URL for `Signature-Agent`. | `https://podcastindex.org` |
| `AGGRIVATOR_SIGNATURE_TTL` | Seconds for the `expires` window. | `300` |

## Error handling (fail-soft — signing never breaks polling)

| Failure | Where | Handling |
|---|---|---|
| `AGGRIVATOR_SIGNING_KEY` unset | startup | Log "signing disabled (no key configured)"; run unsigned. |
| PEM missing/unreadable/not Ed25519 PKCS#8 | `from_pem_file`, startup | Log specific error once; `signer = None`; continue unsigned. |
| `AGGRIVATOR_SIGNATURE_AGENT` not a valid `https://` URL | startup | Log error; disable signing. |
| URL fails to parse | per-request | Skip signing that request (send unsigned); fetch path unaffected. |
| Clock read error | per-request | Skip signing on error rather than panic. |

The signer is validated once at startup, so the per-request path can only ever *skip* signing,
never error out a fetch.

## Testing

1. **Unit tests in `signing.rs`** (no network):
   - `keyid` thumbprint vector from a fixed test PEM, cross-checked against an independent tool.
   - Exact byte-for-byte signature base for a fixed URL + fixed `created/expires`, including the
     non-default-port rule.
   - Round-trip: `sign()` reconstructs a base that `VerifyingKey::verify()` accepts; tampered base fails.
   - Header serialization: `Signature` `sf-binary` form; `Signature-Input` equals the
     `@signature-params` value.
2. **Live verifier (manual/integration):** a `--sign` mode added to `examples/probe.rs` fires a
   signed request at `https://http-message-signatures-example.research.cloudflare.com` and reads
   its valid/invalid verdict — the authoritative external check.
3. **Opt-in regression:** with no env var set, requests are byte-identical to today (no signature
   headers).

## Dependencies (new)

- `ed25519-dalek = { version = "2", features = ["pkcs8","pem","rand_core"] }`
- `sha2 = "0.10"`
- `base64 = "0.22"`
- `serde_json = "1"`
- `rand_core` / `OsRng` used only by the keygen example.

## Limitations / future work

- **Cross-host redirects** carry the initial request's signature (invalid for the new authority);
  the verifier treats that hop as unverified — no worse than today's unsigned request. Same-host
  redirects (the common case) keep a valid signature. Re-signing each hop requires manual redirect
  handling and is deferred.
- **`nonce`** replay protection is omitted in v1.
- **Directory response signing** is a host/serve-time concern, out of scope here.

## Packaging

- Bump to **v0.1.10** (UA becomes `Aggrivator (PodcastIndex.org)/v0.1.10`; still matches the
  `Aggrivator (PodcastIndex.org)/v*` pattern in `BOT.md` / `ips.json`).
- Add a worklog entry to `README.md`.
- Add a short Web Bot Auth section to `BOT.md` pointing at the directory path, since signing and
  the published directory are two halves of the same verification story.
