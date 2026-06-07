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

#[cfg(test)]
mod tests {
    use super::*;

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
}
