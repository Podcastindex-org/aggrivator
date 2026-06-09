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
use ed25519_dalek::pkcs8::EncodePrivateKey;
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
        .to_pkcs8_pem(Default::default())
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
