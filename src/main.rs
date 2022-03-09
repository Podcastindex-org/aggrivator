extern crate futures;
extern crate rusqlite;
extern crate chrono;
extern crate reqwest;

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::time::{ SystemTime, UNIX_EPOCH };
use rusqlite::{ Connection };
use reqwest::header;
use futures::StreamExt;
use chrono::NaiveDateTime;
use httpdate;



//##: Global definitions
static USERAGENT: &str = "Aggrivator (PodcastIndex.org)/v0.1-alpha[dave@podcastindex.org]";
struct Podcast {
    id: u64,
    url: String,
    title: String,
    last_update: u64,
    etag: String
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
    etag: String
}

//##: Implement
impl fmt::Display for HydraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fatal error: {}", self.0)
    }
}
impl Error for HydraError {}


//##: -------------------- Main() -----------------------
//##: ---------------------------------------------------
#[tokio::main]
async fn main() {
    //Globals
    //let pi_database_url: &str = "https://cloudflare-ipfs.com/ipns/k51qzi5uqu5dkde1r01kchnaieukg7xy9i6eu78kk3mm3vaa690oaotk1px6wo/podcastindex_feeds.db.tgz";
    let sqlite_file: &str = "podcastindex_feeds.db";

    //Fetch urls
    let podcasts = get_feeds_from_sql(sqlite_file);
    match podcasts {
        Ok(podcasts) => {
            match fetch_feeds(podcasts).await {
                Ok(_) => {

                },
                Err(_) => {

                }
            }
        },
        Err(e) => println!("{}", e),
    }
}
//##: ---------------------------------------------------


//##: Take in a vector of Podcasts and attempt to pull each one of them that is update
async fn fetch_feeds(podcasts: Vec<Podcast>) -> Result<(), Box<dyn std::error::Error>> {
    let fetches = futures::stream::iter(
        podcasts.into_iter().map(|podcast| {
            async move {
                match check_feed_is_updated(&podcast.url, &podcast.etag.as_str(), podcast.last_update, podcast.id).await {
                    Ok(resp) => {
                        match resp {
                            true => println!("  Feed: [{}|{}|{}] is updated.", podcast.id, podcast.title, podcast.url),
                            false => println!("  Feed: [{}|{}|{}] is NOT updated.", podcast.id, podcast.title, podcast.url),
                        }
                    }
                    Err(e) => println!("ERROR downloading: [{}], {:#?}", podcast.url, e),
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
    let since_time: u64 = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => n.as_secs() - (86400 * 90),
        Err(_) => panic!("SystemTime before UNIX EPOCH!"),
    };

    //Connect to the PI sqlite database file
    let sql = Connection::open(sqlite_file);
    match sql {
        Ok(sql) => {
            println!("----- Got some podcasts. -----\n");

            //Run the query and store the result
            let sql_text: String = format!("SELECT id, url, title, lastUpdate, etag \
                                            FROM podcasts \
                                            WHERE url NOT LIKE 'https://anchor.fm%' \
                                              AND newestItemPubdate > {} \
                                            ORDER BY id ASC \
                                            LIMIT 5", since_time);
            let stmt = sql.prepare(sql_text.as_str());
            match stmt {
                Ok(mut dbresults) => {
                    let podcast_iter = dbresults.query_map([], |row| {
                        Ok(Podcast {
                            id: row.get(0).unwrap(),
                            url: row.get(1).unwrap(),
                            title: row.get(2).unwrap(),
                            last_update: row.get(3).unwrap(),
                            etag: row.get(4).unwrap(),
                        })
                    }).unwrap();

                    //Iterate the list and store
                    for podcast in podcast_iter {
                        let pod: Podcast = podcast.unwrap();
                        podcasts.push(pod);
                    }
                },
                Err(e) => return Err(Box::new(HydraError(format!("Error running SQL query: [{}]", e).into())))
            }

            //sql.close();

            return Ok(podcasts);
        },
        Err(e) => return Err(Box::new(HydraError(format!("Error running SQL query: [{}]", e).into())))
    }
}


//##: Do a conditional request if possible, using the etag and last-modified values from the previous run
async fn check_feed_is_updated(url: &str, etag: &str, last_update: u64, feed_id: u64) -> Result<bool, Box<dyn Error>> {
    //let mut podcast_check_result: PodcastCheckResult;

    //Build the initial query headers
    let mut headers = header::HeaderMap::new();
    headers.insert("User-Agent", header::HeaderValue::from_static(USERAGENT));

    //Create an http header compatible timestamp value to send with the conditional request based on
    //the `last_update` of the feed we're checking
    if last_update > 0 {
        let dt = NaiveDateTime::from_timestamp(last_update as i64, 0);
        let if_modified_since_time = dt.format("%a,%e %b %Y %H:%M:%S UTC").to_string();
        println!("  If-Modified-Since: {:?}", if_modified_since_time);
        headers.insert("If-Modified-Since", header::HeaderValue::from_str(if_modified_since_time.as_str()).unwrap());
    }

    //Create an http header compatible etag value to send with the conditional request based on
    //the `etag` of the feed we're checking
    if !etag.is_empty() {
        println!("  If-None-Match: {:?}", etag);
        headers.insert("If-None-Match", header::HeaderValue::from_str(etag).unwrap());
    }

    //Build the query client
    let client = reqwest::Client::builder().default_headers(headers).build().unwrap();

    //Send the request and display the results or the error
    let response = client.get(url).send().await;
    match response {
        Ok(res) => {
            println!("  Response Status: [{}]", res.status());
            let response_http_status = res.status().as_u16();

            let mut r_etag = "".to_string();
            let mut r_modified = last_update;

            //If a 304 was returned we know it's not changed
            if response_http_status == 304 {
                return Ok(false);
            }

            //Permanent redirect
            if response_http_status == 301 {

            }

            //Temporary redirect
            if response_http_status == 302 {

            }

            //Change detection using headers
            for (key,val) in res.headers().into_iter() {
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

            //Body returned
            if response_http_status == 200 {
                let body = res.text_with_charset("utf-8").await?; //TODO: handle errors
                let file_name = format!("feeds/{}_{}.txt", feed_id, response_http_status);
                let mut feed_file = File::create(file_name)?;
                feed_file.write_all(format!("{}\n", r_modified).as_bytes());
                feed_file.write_all(format!("{}\n", r_etag).as_bytes());
                feed_file.write_all(body.as_bytes())?;
                println!("  - Body downloaded.");
                Ok(true)
            } else {
                Ok(false)
            }
        },
        Err(e) => {
            eprintln!("Error: [{}]", e);
            return Err(Box::new(HydraError(format!("Error running SQL query: [{}]", e).into())));
        }
    }
}