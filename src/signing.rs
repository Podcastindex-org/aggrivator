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

#[cfg(test)]
mod tests {
    use super::*;

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
}
