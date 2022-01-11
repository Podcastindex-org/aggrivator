use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::time::SystemTime;
use rusqlite::{ Connection };
use reqwest::header;
use chrono::prelude::*;
use futures::StreamExt;


//##: Global definitions
static USERAGENT: &str = "Aggrivator (PodcastIndex.org)/v0.1-alpha[dave@podcastindex.org]";
struct Podcast {
    id: u64,
    url: String,
    title: String,
    last_update: i64,
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
            // for podcast in podcasts {
            //     println!("-----------------------");
            //     println!("{:#?}|{:#?}|{:#?}|{:#?}", podcast.id, podcast.url, podcast.title, podcast.last_update);
            //     match check_feed_is_updated(podcast.url.as_str(), podcast.etag.as_str(), podcast.last_update) {
            //         Ok(updated) => {
            //             if updated {
            //                 println!("  Podcast: [{}] updated.", podcast.url);
            //             } else {
            //                 println!("  Podcast: [{}] NOT updated.", podcast.url);
            //             }
            //         },
            //         Err(e) => {
            //             eprintln!("Error checking feed: {:?}", e);
            //         }
            //     };
            // }
            fetch_feeds(podcasts).await;
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
                            true => println!("  Feed: [{}] is updated.", podcast.url),
                            false => println!("  Feed: [{}] is NOT updated.", podcast.url),
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
    let since_time: u64 = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_secs() - (86400 * 90),
        Err(_) => panic!("SystemTime before UNIX EPOCH!"),
    };

    //Connect to the PI sqlite database file
    let sql = Connection::open(sqlite_file);
    match sql {
        Ok(sql) => {
            println!("Got some podcasts.");

            //Run the query and store the result
            let sql_text: String = format!("SELECT id, url, title, lastUpdate \
                                            FROM podcasts \
                                            WHERE url NOT LIKE 'https://anchor.fm%' \
                                              AND newestItemPubdate > {} \
                                            ORDER BY id DESC \
                                            LIMIT 1", since_time);
            let stmt = sql.prepare(sql_text.as_str());
            match stmt {
                Ok(mut dbresults) => {
                    let podcast_iter = dbresults.query_map([], |row| {
                        Ok(Podcast {
                            id: row.get(0).unwrap(),
                            url: row.get(1).unwrap(),
                            title: row.get(2).unwrap(),
                            last_update: row.get(3).unwrap(),
                            etag: "".to_string()
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


fn mark_feed_as_updated(sqlite_file: &str, podcast_check_result: PodcastCheckResult) -> Result<bool, Box<dyn Error>> {
    return Ok(true)
}


// set_feed_status_in_sql(sqlite_file: &str, <Podcast>) -> Result<bool, Box<dyn Error>> {



// }


//##: Fetch the content of a url
fn fetch_feed(url: &str) -> Result<bool, Box<dyn Error>> {
    let feed_url: &str = url;

    //##: Build the query with the required headers
    let mut headers = header::HeaderMap::new();
    headers.insert("User-Agent", header::HeaderValue::from_static(USERAGENT));
    let client = reqwest::blocking::Client::builder().default_headers(headers).build().unwrap();

    //##: Send the request and display the results or the error
    let res = client.get(feed_url).send();
    match res {
        Ok(res) => {
            println!("Response Status: [{}]", res.status());
            println!("Response Body: {}", res.text().unwrap());
            return Ok(true);
        },
        Err(e) => {
            eprintln!("Error: [{}]", e);
            return Err(Box::new(HydraError(format!("Error running SQL query: [{}]", e).into())));
        }
    }
}


//##: Do a head check on a url to see if it's been modified
async fn check_feed_is_updated(url: &str, etag: &str, last_update: i64, feed_id: u64) -> Result<bool, Box<dyn Error>> {
    let mut podcast_check_result: PodcastCheckResult;

    //Get the current datetime
    //let now: DateTime<Utc> = Utc::now();
    // let dt = Utc.ymd(2021, 7, 8).and_hms(9, 10, 11);
    let dt = NaiveDateTime::from_timestamp(last_update, 0);
    let if_modified_since_time = dt.format("%a,%e %b %Y %H:%M:%S UTC").to_string();
    println!("  If-Modified-Since: {:?}", if_modified_since_time);

    //##: Build the query with the required headers
    let mut headers = header::HeaderMap::new();
    headers.insert("User-Agent", header::HeaderValue::from_static(USERAGENT));
    headers.insert("If-Modified-Since", header::HeaderValue::from_str(if_modified_since_time.as_str()).unwrap());
    let client = reqwest::Client::builder().default_headers(headers).build().unwrap();

    //##: Send the request and display the results or the error
    let response = client.get(url).send().await;
    match response {
        Ok(res) => {
            println!("Response Status: [{}]", res.status());
            let response_http_status = res.status().as_u16();

            //If a 304 was returned we know it's not changed
            if response_http_status == 304 {
                return Ok(false);
            }

            //Change detection using headers
            for (key,val) in res.headers().into_iter() {
                if key == "last-modified" {
                    println!("  Last-Modified: {:?}", val);
                }
                if key == "etag" {
                    println!("  ETag: {:?}", val);
                    if etag == val {
                        return Ok(false);
                    }
                }
            }

            //Body returned
            if response_http_status == 200 {
                let body = res.text_with_charset("utf-8").await?; //TODO: handle errors
                let file_name = format!("{}_{}.txt", feed_id, response_http_status);
                let mut feed_file = File::create(file_name)?;
                feed_file.write_all(body.as_bytes())?;
                println!("{}", body);
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