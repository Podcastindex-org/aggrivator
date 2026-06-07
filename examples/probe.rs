// Standalone probe to reproduce/diagnose feed-fetch failures (e.g. the 415 from
// nakedbiblepodcast.com) locally, using the IDENTICAL reqwest client config as
// the production poller in src/main.rs::check_feed_is_updated.
//
// Why an example binary: it links the same reqwest (rustls-tls + gzip) build, so
// the TLS fingerprint, header set, redirect handling and gzip behavior match prod
// exactly -- something `curl` cannot reproduce (curl uses a different TLS stack,
// which Cloudflare bot-management fingerprints differently).
//
// Usage:
//   cargo run --example probe -- <url>
//
//   # Toggle an Accept header to A/B test the suspected fix:
//   ACCEPT='application/rss+xml, application/xml;q=0.9, */*;q=0.8' \
//     cargo run --example probe -- https://nakedbiblepodcast.com/feed/podcast/
//
//   # Replay a conditional request like prod would after a prior fetch:
//   IF_NONE_MATCH='"abc123"' cargo run --example probe -- <url>
//   IF_MODIFIED_SINCE='Wed, 01 Jan 2025 00:00:00 GMT' cargo run --example probe -- <url>
//
// Env vars (all optional):
//   ACCEPT             value for the Accept header (unset => no Accept header, matching prod today)
//   IF_NONE_MATCH      value for If-None-Match (conditional request)
//   IF_MODIFIED_SINCE  value for If-Modified-Since (conditional request)
//   UA                 override User-Agent (default = same as prod)

use std::env;
use std::time::Duration;
use reqwest::{header, redirect};

const DEFAULT_USERAGENT: &str = concat!("Aggrivator (PodcastIndex.org)/v", env!("CARGO_PKG_VERSION"));

#[tokio::main]
async fn main() {
    let url = match env::args().nth(1) {
        Some(u) => u,
        None => {
            eprintln!("usage: cargo run --example probe -- <url>");
            std::process::exit(2);
        }
    };

    let user_agent = env::var("UA").unwrap_or_else(|_| DEFAULT_USERAGENT.to_string());

    // Build the request headers exactly like prod does, plus the optional toggles.
    let mut headers = header::HeaderMap::new();
    headers.insert("User-Agent", header::HeaderValue::from_str(&user_agent).unwrap());

    if let Ok(accept) = env::var("ACCEPT") {
        headers.insert("Accept", header::HeaderValue::from_str(&accept).unwrap());
        println!("  + Accept: {}", accept);
    } else {
        println!("  (no Accept header -- matches current production behavior)");
    }
    if let Ok(inm) = env::var("IF_NONE_MATCH") {
        headers.insert("If-None-Match", header::HeaderValue::from_str(&inm).unwrap());
        println!("  + If-None-Match: {}", inm);
    }
    if let Ok(ims) = env::var("IF_MODIFIED_SINCE") {
        headers.insert("If-Modified-Since", header::HeaderValue::from_str(&ims).unwrap());
        println!("  + If-Modified-Since: {}", ims);
    }

    // Same custom redirect policy shape as prod: log each hop, cap at 10.
    let custom = redirect::Policy::custom(move |attempt| {
        let status_code = attempt.status().as_u16();
        println!("  -> redirect [{}] to {}", status_code, attempt.url());
        if attempt.previous().len() > 9 {
            return attempt.error("Error - Too many redirects");
        }
        attempt.follow()
    });

    // IDENTICAL client builder to src/main.rs::check_feed_is_updated.
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(20))
        .default_headers(headers)
        .gzip(true)
        .redirect(custom)
        .build()
        .unwrap();

    println!("\nUser-Agent: {}", user_agent);
    println!("GET {}\n", url);

    match client.get(&url).send().await {
        Ok(res) => {
            let status = res.status();
            let final_url = res.url().to_string();
            println!("==> Response Status: {}", status);
            println!("==> Final URL:       {}", final_url);
            println!("==> Response headers:");
            for (k, v) in res.headers().iter() {
                println!("      {}: {}", k, v.to_str().unwrap_or("<binary>"));
            }
            match res.text().await {
                Ok(body) => println!("\n==> Body length: {} bytes", body.len()),
                Err(e) => println!("\n==> Error reading body: {}", e),
            }
        }
        Err(e) => {
            eprintln!("==> Request error: {}", e);
            std::process::exit(1);
        }
    }
}
