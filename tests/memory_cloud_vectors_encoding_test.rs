//! The memory-cloud vectors blob is served without content encoding.
//!
//! The blob is little-endian f32: brotli or gzip save about 8% of its
//! 46 MB at about 0.5 s of server CPU per request (measured 2026-10-08,
//! 30,000 x 384 snapshot: identity 0.05 s, gzip 0.57 s, br 0.50 s direct).
//! `main.rs` wraps the whole app in `middleware::Compress`, so the handler
//! must opt out explicitly. This test runs the response through the same
//! middleware with the `Accept-Encoding` a browser sends.

use actix_web::http::header::{ACCEPT_ENCODING, CONTENT_ENCODING};
use actix_web::{middleware, test, web, App, HttpResponse};
use bytes::Bytes;
use visionclaw_server::handlers::memory_cloud_handler::vectors_response;

const DIM: usize = 384;
const COUNT: usize = 64;

fn blob() -> Bytes {
    // Varied floats so a compressor would have work to do.
    let mut out = Vec::with_capacity(DIM * COUNT * 4);
    for i in 0..DIM * COUNT {
        out.extend_from_slice(&((i as f32 * 0.37).sin()).to_le_bytes());
    }
    Bytes::from(out)
}

async fn serve() -> HttpResponse {
    vectors_response(blob(), DIM, COUNT)
}

#[actix_web::test]
async fn vectors_blob_bypasses_compress_middleware() {
    let app = test::init_service(
        App::new()
            .wrap(middleware::Compress::default())
            .route("/v", web::get().to(serve)),
    )
    .await;

    for accept in ["gzip, deflate, br, zstd", "br", "gzip", "zstd"] {
        let req = test::TestRequest::get()
            .uri("/v")
            .insert_header((ACCEPT_ENCODING, accept))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert!(res.status().is_success(), "status for {accept}");
        let enc = res
            .headers()
            .get(CONTENT_ENCODING)
            .map(|v| v.to_str().unwrap().to_owned());
        assert!(
            matches!(enc.as_deref(), None | Some("identity")),
            "Accept-Encoding {accept}: blob was encoded as {enc:?}"
        );
        assert_eq!(
            res.headers().get("cache-control").unwrap(),
            "no-store",
            "no-store must survive"
        );
        assert_eq!(res.headers().get("x-memory-cloud-dim").unwrap(), "384");
        assert_eq!(res.headers().get("x-memory-cloud-count").unwrap(), "64");
        assert_eq!(
            res.headers().get("content-type").unwrap(),
            "application/octet-stream"
        );
        let body = test::read_body(res).await;
        assert_eq!(body, blob(), "body must be the raw blob, byte for byte");
    }
}
