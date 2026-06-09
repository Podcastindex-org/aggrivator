extern crate futures;
extern crate rusqlite;
extern crate chrono;
extern crate reqwest;

use std::error::Error;
use std::fmt;
use std::fs::File;
//use std::fs::create_dir;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use rusqlite::{Connection};
use reqwest::{header, redirect};
use futures::StreamExt;
use httpdate;
use std::sync::Arc;
use aggrivator::signing::WebBotAuthSigner;



//##: Global definitions
const USERAGENT: &str = concat!("Aggrivator (PodcastIndex.org)/v", env!("CARGO_PKG_VERSION"));
//70 megabytes
const MAX_BODY_LENGTH: usize = 73400320; 
//static DIR_FEED_FILES: &str = "feeds";
//static DIR_REDIRECT_FILES: &str = "redirects";
const ERRORCODE_GENERAL_CONNECTION_FAILURE: u16 = 666;
const ERRORCODE_GENERAL_DOWNLOAD_FAILURE: u16 = 667;
const ERRORCODE_GENERAL_FILE_SIZE_EXCEEDED: u16 = 668;


struct Podcast {
    id: u64,
    url: String,
    title: String,
    last_modified: u64,
    etag: String,
}

#[derive(Debug)]
struct HydraError(String);

#[allow(dead_code)]
struct PodcastCheckResult {
    id: u64,
    url: String,
    updated: bool,
    status_code: u16,
    last_modified_string: String,
    last_modified_timestamp: u64,
    etag: String,
}

//##: Implement
impl fmt::Display for HydraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fatal error: {}", self.0)
    }
}

impl Error for HydraError {}


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


//##: -------------------- Main() -----------------------
//##: ---------------------------------------------------
#[tokio::main]
async fn main() {
    //Globals
    let sqlite_file: &str = "feed_poller_queue.db";

    //Make sure folders we need exist.
    // TODO: need env and error intelligence for containerizing
    // match create_dir(DIR_FEED_FILES) {
    //     Ok()
    // }
    // create_dir(DIR_REDIRECT_FILES)?;

    //Announce what we are
    println!("{}", USERAGENT);
    println!("{}\n", "-".repeat(USERAGENT.len()));

    //Fetch urls
    let signer = build_signer();
    let podcasts = get_feeds_from_sql(sqlite_file);
    match podcasts {
        Ok(podcasts) => {
            match fetch_feeds(podcasts, signer).await {
                Ok(_) => {}
                Err(_) => {}
            }
        }
        Err(e) => println!("{}", e),
    }
}
//##: ---------------------------------------------------


//##: Take in a vector of Podcasts and attempt to pull each one of them that is update
async fn fetch_feeds(
    podcasts: Vec<Podcast>,
    signer: Option<Arc<WebBotAuthSigner>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let fetches = futures::stream::iter(
        podcasts.into_iter().map(|podcast| {
            let signer = signer.clone();
            async move {
                match check_feed_is_updated(&podcast.url, &podcast.etag.as_str(), podcast.last_modified, podcast.id, signer.as_deref()).await {
                    Ok(resp) => {
                        match resp {
                            true => println!("  Feed: [{}|{}|{}] is updated.", podcast.id, podcast.title, podcast.url),
                            false => println!("  Feed: [{}|{}|{}] is NOT updated.", podcast.id, podcast.title, podcast.url),
                        }
                    }
                    Err(e) => {
                        println!("ERROR downloading: [{}], {:#?}", podcast.url, e);
                        if let Err(e) = write_feed_file(
                            podcast.id,
                            ERRORCODE_GENERAL_DOWNLOAD_FAILURE,
                            0,
                            "".to_string(),
                            podcast.url,
                            &"".to_string()
                        ) {
                            eprintln!("Error writing download error feed file: {:#?}", e);
                        }
                    },
                }
            }
        })
    ).buffer_unordered(100).collect::<Vec<()>>();
    fetches.await;
    Ok(())
}


//##: Get a list of podcasts from the downloaded sqlite db
fn get_feeds_from_sql(sqlite_file: &str) -> Result<Vec<Podcast>, Box<dyn Error>> {
    //Locals
    let mut podcasts: Vec<Podcast> = Vec::new();

    //Restrict to feeds that have updated in a reasonable amount of time
    let _since_time: u64 = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => n.as_secs() - (86400 * 90),
        Err(_) => panic!("SystemTime before UNIX EPOCH!"),
    };

    //Connect to the PI sqlite database file
    let sql = Connection::open(sqlite_file);
    match sql {
        Ok(sql) => {
            println!("----- Got some podcasts. -----\n");

            //Run the query and store the result
            let sql_text: String = format!("SELECT id, \
                                                   url, \
                                                   title, \
                                                   lastmod, \
                                                   etag \
                                            FROM podcasts \
                                            ORDER BY id ASC");
            let stmt = sql.prepare(sql_text.as_str());
            match stmt {
                Ok(mut dbresults) => {
                    let podcast_iter = dbresults.query_map([], |row| {
                        Ok(Podcast {
                            id: row.get(0).unwrap(),
                            url: row.get(1).unwrap(),
                            title: row.get(2).unwrap(),
                            last_modified: row.get(3).unwrap(),
                            etag: row.get(4).unwrap(),
                        })
                    }).unwrap();

                    //Iterate the list and store
                    for podcast in podcast_iter {
                        let pod: Podcast = podcast.unwrap();
                        podcasts.push(pod);
                    }
                }
                Err(e) => return Err(Box::new(HydraError(format!("Error running SQL query: [{}]", e).into())))
            }

            //sql.close();

            return Ok(podcasts);
        }
        Err(e) => return Err(Box::new(HydraError(format!("Error running SQL query: [{}]", e).into())))
    }
}


//##: Do a conditional request if possible, using the etag and last-modified values from the previous run
async fn check_feed_is_updated(url: &str, etag: &str, last_modified: u64, feed_id: u64, signer: Option<&WebBotAuthSigner>) -> Result<bool, Box<dyn Error>> {
    //let mut podcast_check_result: PodcastCheckResult;

    //Build the initial query headers
    let mut headers = header::HeaderMap::new();
    headers.insert("User-Agent", header::HeaderValue::from_static(USERAGENT));

    //Advertise the feed formats we actually want. Some origins/WAFs reject requests
    //that send no Accept header (reqwest sends none by default) with a 415; a real
    //browser always sends one. This won't defeat IP-based bot challenges, but it
    //fixes feeds whose front-end requires a sane Accept.
    headers.insert("Accept", header::HeaderValue::from_static(
        "application/rss+xml, application/atom+xml, application/xml;q=0.9, text/xml;q=0.9, */*;q=0.8"
    ));

    //Create an http header compatible timestamp value to send with the conditional request based on
    //the `last_modified` of the feed we're checking
    if last_modified > 0 {
        let ts_secs = Duration::from_secs(last_modified);
        let ts = SystemTime::UNIX_EPOCH.checked_add(ts_secs).unwrap();
        let if_modified_since_time = httpdate::fmt_http_date(ts);
        println!("  [{}|{}] If-Modified-Since: {:?}", feed_id, last_modified, if_modified_since_time);
        headers.insert("If-Modified-Since", header::HeaderValue::from_str(if_modified_since_time.as_str()).unwrap());
    }

    //Create an http header compatible etag value to send with the conditional request based on
    //the `etag` of the feed we're checking
    if !etag.is_empty() {
        println!("  [{}] If-None-Match: {:?}", feed_id, etag);
        headers.insert("If-None-Match", header::HeaderValue::from_str(etag).unwrap());
    }

    //Custom redirect policy so we can intercept redirect requests
    let feed_id2 = feed_id.clone();
    let custom = redirect::Policy::custom(move |attempt| {
        let status_code = attempt.status().as_u16();

        //Bail out if we reach 10 or more redirects
        if attempt.previous().len() > 9 {
            return attempt.error("Error - Too many redirects");
        }

        //If this is a permanent redirect, drop a stub file so that the parser can come by later
        //and pick up these url changes
        if status_code == 301 || status_code == 308 {
            if let Err(e) = write_feed_file(
                feed_id2,
                status_code,
                0,
                "".to_string(),
                attempt.url().to_string(),
                &"".to_string()
            ) {
                eprintln!("Error writing redirect file: {:#?}", e);
            }
        }

        //Keep going
        attempt.follow()
    });

    //Build the query client
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

    //Default response header values to use in case we can't get something during
    //the request. These are safe fallbacks.
    let mut r_etag = "[[NO_ETAG]]".to_string();
    let mut r_modified = last_modified;
    let mut r_url = url.to_string();

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
    match response {
        Ok(res) => {
            println!("  Response Status: [{}]", res.status());
            let response_http_status = res.status().as_u16();

            //Default header values
            r_etag = "[[NO_ETAG]]".to_string();
            r_modified = last_modified;
            r_url = res.url().to_string();

            //Change detection using headers
            for (key, val) in res.headers().into_iter() {
                if key == "last-modified" && !val.is_empty() {
                    println!("  Last-Modified: {:#?}", val);

                    //See if we can get a parseable date-time string from the header value, and if
                    //so, try to parse that to a unix epoch value for storing
                    if let Ok(headerval) = val.to_str() {
                        if let Ok(timestamp) = httpdate::parse_http_date(headerval) {
                            if let Ok(systime) = timestamp.duration_since(UNIX_EPOCH) {
                                r_modified = systime.as_secs();
                                println!("  r_modified: {:#?}", r_modified);
                            }
                        }
                    }
                }
                if key == "etag" && !val.is_empty() {
                    println!("  ETag: {:#?}", val);

                    //If there is a sane value here, that's our guy so we extract it
                    if let Ok(headerval) = val.to_str() {
                        r_etag = headerval.to_string();
                    }
                }
            }

            //Take appropriate action depending on the response status
            let mut body = "".to_string();
            match response_http_status {
                //Standard OK (perhaps with a transform) - response body included
                200 | 203 | 214 => {
                    body = res.text_with_charset("utf-8").await?; //TODO: handle errors
                    if let Err(e) = write_feed_file(feed_id, response_http_status, r_modified, r_etag, r_url, &body) {
                        eprintln!("Error writing OK feed file: {:#?}", e);
                    }
                    println!("  - Content downloaded.");
                    return Ok(true);
                },
                //No content - no response body
                204 => {
                    if let Err(e) = write_feed_file(feed_id, response_http_status, r_modified, r_etag, r_url, &body) {
                        eprintln!("Error writing 204 feed file: {:#?}", e);
                    }
                    println!("  - No content.");
                    return Ok(true);
                },
                //Content not modified - no response body
                304 => {
                    if let Err(e) = write_feed_file(feed_id, response_http_status, r_modified, r_etag, r_url, &body) {
                        eprintln!("Error writing 304 feed file: {:#?}", e);
                    }
                    println!("  - Content not modified.");
                    return Ok(false);
                },
                //Request error - no response body
                400..=499 => {
                    if let Err(e) = write_feed_file(feed_id, response_http_status, r_modified, r_etag, r_url, &body) {
                        eprintln!("Error writing client error feed file: {:#?}", e);
                    }
                    println!("  - Request error.");
                    return Ok(false);
                },
                //Server error - no response body
                500..=999 => {
                    if let Err(e) = write_feed_file(feed_id, response_http_status, r_modified, r_etag, r_url, &body) {
                        eprintln!("Error writing server feed file: {:#?}", e);
                    }
                    println!("  - Server error.");
                    return Ok(false);
                },
                //Something else that we don't handle
                _ => {
                    if let Err(e) = write_feed_file(feed_id, response_http_status, r_modified, r_etag, r_url, &body) {
                        eprintln!("Error writing server feed file: {:#?}", e);
                    }
                    println!("  - Unhandled status code.");
                    return Ok(false);
                }
            }
        }
        Err(e) => {
            eprintln!("Error: [{}]", e);
            if let Err(e) = write_feed_file(
                feed_id,
                ERRORCODE_GENERAL_CONNECTION_FAILURE,
                r_modified,
                r_etag,
                r_url,
                &"".to_string()
            ) {
                eprintln!("Error writing connection error feed file: {:#?}", e);
            }
            return Err(Box::new(HydraError(format!("Error downloading feed: [{}]", e).into())));
        }
    }
}


//Write a feed file out to the filesystem with metadata and body
fn write_feed_file(feed_id: u64, status_code: u16, r_modified: u64, r_etag: String, r_url: String, body: &String)
    -> Result<bool, Box<dyn Error>>
{
    //Holds the current status code, which can change later
    let mut status_code_prefix = status_code;

    //What time is it now
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();

    //If the body exceeds the maximum we're willing to handle, rename the file as an error code file
    let body_length =  body.len();
    println!("Body length: {}\n", body_length);
    if body_length > MAX_BODY_LENGTH {
        status_code_prefix = ERRORCODE_GENERAL_FILE_SIZE_EXCEEDED;
    }

    //What directory to place this file in
    let mut directory = "feeds";
    if status_code_prefix == 301 || status_code_prefix == 308 {
        directory = "redirects";
    }

    //The filename is the feed id and the http response status
    let file_name = format!("{}/{}_{}.txt", directory, feed_id, status_code_prefix);

    //Create the file TODO: Needs error checking on these unwraps
    let mut feed_file = File::create(file_name)?;
    feed_file.write_all(format!("{}\n", r_modified).as_bytes())?;
    feed_file.write_all(format!("{}\n", r_etag).as_bytes())?;
    feed_file.write_all(format!("{}\n", r_url).as_bytes())?;
    feed_file.write_all(format!("{}\n", now).as_bytes())?;

    //If the body was too long, don't write it
    if status_code_prefix != ERRORCODE_GENERAL_FILE_SIZE_EXCEEDED {
        feed_file.write_all(body.as_bytes())?;
    }


    Ok(true)
}