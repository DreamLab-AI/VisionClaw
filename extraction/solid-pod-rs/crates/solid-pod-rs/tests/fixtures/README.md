# JSS interop fixtures

Each pair `<name>.request.http` / `<name>.response.http` documents a
request/response cycle we expect to reproduce exactly.

File format: the first line is `METHOD path` (request) or `HTTP/1.1
STATUS REASON` (response). Subsequent lines are headers until the
blank line. An optional body follows. Headers that must be compared
literally are prefixed with `X-CompareMode: literal`; headers that
may vary (ETag, Last-Modified) are prefixed with `X-CompareMode:
allow`.

The harness in `../interop_jss.rs` reads these fixtures via
`parse_http_file()` and drives them against the in-crate `Storage`
trait, then diffs the response bytes against the captured response.
