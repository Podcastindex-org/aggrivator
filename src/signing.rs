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
#[derive(Debug)]
pub struct WebBotAuthSigner {
    signing_key: SigningKey,
    keyid: String,
    signature_agent: String,
    ttl_secs: u64,
}

/// RFC 7638 JWK thumbprint of an Ed25519 public key, base64url-no-pad.
fn compute_keyid(public_key: &[u8; 32]) -> String {
    let x = URL_SAFE_NO_PAD.encode(public_key);
    // Members in lexicographic order, no whitespace, per RFC 7638.
    let canonical = format!(r#"{{"crv":"Ed25519","kty":"OKP","x":"{}"}}"#, x);
    let digest = Sha256::digest(canonical.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

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

/// The RFC 9421 signature parameters string: the inner component list plus
/// parameters, in the canonical order from the web-bot-auth draft worked example
/// (created, keyid, alg, expires, tag). Verified byte-correct against Cloudflare's
/// reference verifier. This exact string appears both in `Signature-Input` (after
/// `sig1=`) and as the `@signature-params` value.
fn signature_params(keyid: &str, created: u64, expires: u64) -> String {
    format!(
        "(\"@authority\" \"signature-agent\");created={};keyid=\"{}\";alg=\"ed25519\";expires={};tag=\"web-bot-auth\"",
        created, keyid, expires
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

impl WebBotAuthSigner {
    /// Load a PKCS#8 Ed25519 private key from a PEM file. Validates that
    /// `signature_agent` is an `https://` URL and precomputes the keyid.
    pub fn from_pem_file(
        path: &str,
        signature_agent: String,
        ttl_secs: u64,
    ) -> Result<Self, Box<dyn Error>> {
        // Validate the signature agent up front (before any I/O) so a successfully
        // constructed signer can never fail when building request headers in `sign`.
        let parsed = Url::parse(&signature_agent)?;
        if parsed.scheme() != "https" {
            return Err(format!("signature agent must be https: {}", signature_agent).into());
        }
        if !signature_agent.is_ascii() {
            return Err(format!("signature agent must be ASCII: {}", signature_agent).into());
        }
        let pem = std::fs::read_to_string(path)?;
        let signing_key = SigningKey::from_pkcs8_pem(&pem)?;
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
        let expires = now_unix.saturating_add(self.ttl_secs);
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

#[cfg(test)]
mod tests {
    use super::*;

    const KID: &str = "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k";

    #[test]
    fn signature_params_exact() {
        let params = signature_params(KID, 1735689600, 1735689900);
        assert_eq!(
            params,
            "(\"@authority\" \"signature-agent\");created=1735689600;keyid=\"kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k\";alg=\"ed25519\";expires=1735689900;tag=\"web-bot-auth\""
        );
    }

    #[test]
    fn signature_base_exact() {
        let params = signature_params(KID, 1735689600, 1735689900);
        let base = build_signature_base("example.com", "\"https://podcastindex.org\"", &params);
        let expected = "\"@authority\": example.com\n\
\"signature-agent\": \"https://podcastindex.org\"\n\
\"@signature-params\": (\"@authority\" \"signature-agent\");created=1735689600;keyid=\"kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k\";alg=\"ed25519\";expires=1735689900;tag=\"web-bot-auth\"";
        assert_eq!(base, expected);
    }

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

    #[test]
    fn sign_emits_verifiable_signature() {
        use ed25519_dalek::{Signature, Verifier};
        use rand_core::OsRng;
        use std::convert::TryInto;

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

    #[test]
    fn from_pem_file_rejects_non_https_agent() {
        let err = WebBotAuthSigner::from_pem_file(
            "/nonexistent.pem",
            "http://podcastindex.org".to_string(),
            300,
        )
        .unwrap_err();
        assert!(err.to_string().contains("https"));
    }

    #[test]
    fn from_pem_file_rejects_non_ascii_agent() {
        let err = WebBotAuthSigner::from_pem_file(
            "/nonexistent.pem",
            "https://exämple.org".to_string(),
            300,
        )
        .unwrap_err();
        assert!(err.to_string().contains("ASCII"));
    }

    #[test]
    fn jwks_has_expected_fields() {
        use rand_core::OsRng;
        let signing_key = SigningKey::generate(&mut OsRng);
        let keyid = compute_keyid(signing_key.verifying_key().as_bytes());
        let signer = WebBotAuthSigner {
            signing_key,
            keyid: keyid.clone(),
            signature_agent: "https://podcastindex.org".to_string(),
            ttl_secs: 300,
        };
        let jwks = signer.jwks();
        let key = &jwks["keys"][0];
        assert_eq!(key["kty"], "OKP");
        assert_eq!(key["crv"], "Ed25519");
        assert_eq!(key["use"], "sig");
        assert_eq!(key["kid"], keyid);
        assert!(!key["x"].as_str().unwrap().is_empty());
    }
}
