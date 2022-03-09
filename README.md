# Aggrivator
The new polling agent for Podcast Index.

This is a Rust rewrite of the aggrivate.js agent currently in production.  The intent is to create a fast, parrallel http polling binary
that is less tightly coupled to the database.



## Worklog

alpha - v0.0.2
 - Writing feed files to disk as "feeds/[feedid]_[httpstatus].txt"
 - The first line of the feed file is the last-modified header
 - The second line is the etag
 - The rest of the file is the feed body

alpha - v0.0.1
 - The basic concepts are working with conditionals and parrallel requests.
 - For now it's just using the publicly downloadable podcastindex database as it's source of feeds.
 - The etag is not something that PI currently stores, so I'm having to add them manually during testing.