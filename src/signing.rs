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
