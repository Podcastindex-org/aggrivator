# Aggrivator — Podcast Index Feed Poller

This document describes the **Aggrivator** crawler operated by [Podcast Index](https://podcastindex.org).
It is published so that site operators and CDNs (e.g. Cloudflare) can identify, verify, and allow-list
the crawler. The canonical, public copy of this file lives in the Aggrivator source repository.

## What it is

Aggrivator is the feed-polling agent for the Podcast Index open podcast directory. Podcast Index is a
free, open index of podcasts. Aggrivator periodically fetches the RSS/Atom feeds that publishers have
submitted to the index so the directory stays current with new episodes and metadata.

It is a well-behaved, read-only crawler. It does **not** scrape page content, render JavaScript, log in,
submit forms, or follow links beyond the specific feed URLs it has been asked to poll.

## Identity

- **Operator:** Podcast Index — https://podcastindex.org
- **User-Agent:** `Aggrivator (PodcastIndex.org)/vX.Y.Z`
  (the `vX.Y.Z` is the current release version, e.g. `Aggrivator (PodcastIndex.org)/v0.1.8`; the
  `Aggrivator (PodcastIndex.org)` prefix is stable across versions)
- **Source code:** this repository (public)

## How it behaves (good-citizen crawler)

- **Read-only:** issues only HTTP `GET` requests to feed URLs. No `POST`/`PUT`/`DELETE`, no writes.
- **Conditional requests:** sends `If-Modified-Since` and `If-None-Match` based on the previously stored
  `Last-Modified` / `ETag`, so unchanged feeds return `304 Not Modified` with no body — minimizing
  bandwidth on both sides.
- **Accept:** advertises feed content types via the `Accept` header
  (`application/rss+xml, application/atom+xml, application/xml;q=0.9, text/xml;q=0.9, */*;q=0.8`).
- **Compression:** accepts `gzip` and decompresses responses.
- **Redirects:** follows redirects, capped at 10 hops; records permanent (`301`/`308`) redirects so the
  index can update the stored feed URL.
- **Timeouts:** 20s connect timeout, 30s overall request timeout.
- **Response size limit:** caps stored feed bodies (currently ~70 MB) to avoid abuse of resources.
- **No JavaScript / no headless browser:** it is a plain HTTP client and cannot solve interactive
  challenges (JS challenges, CAPTCHAs). It simply records the HTTP status it receives.
- **Polite frequency:** feeds are polled on a schedule, not in tight loops against any single host.

## Source IP addresses

Aggrivator's egress IP addresses are published as machine-readable JSON, which is the **single source of
truth** for the addresses it crawls from. Please allow-list these ranges for the Aggrivator User-Agent:

- **`ips.json`:** https://raw.githubusercontent.com/Podcastindex-org/aggrivator/main/ips.json

> The ranges in `ips.json` are authoritative for Aggrivator traffic. Requests claiming the Aggrivator
> User-Agent from any address outside those ranges are not operated by Podcast Index.

## Verification

`ips.json` follows the same `prefixes` shape used by other well-known crawlers (Googlebot, Bingbot), so it
can be consumed by existing tooling:

```json
{
  "prefixes": [
    { "ipv4Prefix": "203.0.113.10/32" },
    { "ipv6Prefix": "2001:db8::/64" }
  ]
}
```

To confirm a request genuinely originates from Aggrivator, check that:

1. the `User-Agent` matches `Aggrivator (PodcastIndex.org)/v*`, **and**
2. the source IP falls within one of the prefixes listed in `ips.json`.

The authoritative copy of `ips.json` lives in this repository and is served directly at the public URL
above, so there is a single file to maintain — update `ips.json` whenever the egress addresses change.

## Web Bot Auth (cryptographic verification)

Aggrivator can cryptographically sign its requests using [Web Bot Auth](https://developers.cloudflare.com/bots/reference/bot-verification/web-bot-auth/)
(RFC 9421 HTTP Message Signatures, Ed25519). Signed requests carry `Signature`, `Signature-Input`, and
`Signature-Agent` headers, with `tag="web-bot-auth"` and a `keyid` that is the RFC 7638 JWK thumbprint
of our published key.

Our public signing key is published as a JSON Web Key Set at:

```
https://podcastindex.org/.well-known/http-message-signatures-directory
```

To verify a request: confirm the `User-Agent` matches `Aggrivator (PodcastIndex.org)/v*`, then verify the
signature against the key whose thumbprint equals the request's `keyid`.

## Contact / abuse

If Aggrivator is causing problems for your site, or you would like it to stop crawling a feed, please
contact Podcast Index via https://podcastindex.org. We will respond promptly to operator requests.

## Why allow-list it

Publishers submit their feeds to Podcast Index specifically so that apps and services can discover their
podcasts. Blocking Aggrivator prevents those publishers' new episodes from appearing across the apps that
rely on the index. Allow-listing the User-Agent and the IP addresses above ensures their feeds stay
current.
