# Aggrivator
The new polling agent for Podcast Index.

This is a Rust rewrite of the aggrivate.js agent currently in production.  The intent is to create a fast, parrallel http polling binary
that is less tightly coupled to the database.



## Worklog

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