# Aggrivator
The new feed polling agent for Podcast Index.

This parallel downloader can pull any sort of non-binary content from a url.  To use it, just load up
the accompanying sqlite database with urls and run it.


## Worklog

v0.1.6
 - Added auto-gzip decompression since some misconfigured servers don't respect accept-encoding

v0.1.5
 - Switched to Rustls-tls as the TLS library
 - Some minor changes to version announcing

beta - v0.1.2
 - File needs to be written even for unhandled status codes

beta - v0.1.1
 - Handle very large responses by setting them as an error code (668)

beta - v0.1.0
 - Including a dummy sqlite db for testing purposes

alpha - v0.0.6
 - Added a timestamp to the file since it's faster this way than slowing down partytime for an fs.stat

alpha - v0.0.5
 - Changed up date building for sending in If-Modified-Since header

alpha - v0.0.4
 - Handles redirects now by dropping a stub file for 301's and 308's with the new url in it
 - A lot of error handling
 - Split file writing into it's own function
 - Added a url line as the 3rd line in each feed file
 - Writes files for 4xx and 5xx errors also

alpha - v0.0.2
 - Writing feed files to disk as "feeds/[feedid]_[httpstatus].txt"
 - The first line of the feed file is the last-modified header
 - The second line is the etag
 - The rest of the file is the feed body

alpha - v0.0.1
 - The basic concepts are working with conditionals and parrallel requests.
 - For now it's just using the publicly downloadable podcastindex database as it's source of feeds.
 - The etag is not something that PI currently stores, so I'm having to add them manually during testing.