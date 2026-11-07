//! Gaussian splat assets (ADR-2170 decision 10).
//!
//! `GET /api/splats` lists the admitted assets and their verified variants;
//! `GET /api/splats/blob/{address}` serves one variant read-only, keyed by its
//! ADR-2023 content address. The store is opened once at boot from
//! `SPLAT_MANIFEST` (default `data/splats/manifest.json`) and `SPLAT_CACHE_DIR`
//! (default `data/splat-cache`); every cached file is re-hashed then, and a
//! mismatch keeps it out of the listing. Variants are produced and admitted
//! offline by `splat-admit` (crates/visionclaw-splat), never by the server.

use actix_files::NamedFile;
use actix_web::http::header::{self, ContentType, HeaderValue};
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use std::path::PathBuf;
use visionclaw_splat::licence::CacheMode;
use visionclaw_splat::manifest::Manifest;
use visionclaw_splat::SplatStore;

const DEFAULT_MANIFEST: &str = "data/splats/manifest.json";
const DEFAULT_CACHE_DIR: &str = "data/splat-cache";

/// Open the store from the environment, re-hashing the cache. Fail-open: a
/// missing or malformed manifest gives an empty listing, not a boot failure.
pub fn open_store_from_env() -> SplatStore {
    let manifest_path =
        std::env::var("SPLAT_MANIFEST").unwrap_or_else(|_| DEFAULT_MANIFEST.to_string());
    let cache_dir = PathBuf::from(
        std::env::var("SPLAT_CACHE_DIR").unwrap_or_else(|_| DEFAULT_CACHE_DIR.to_string()),
    );
    let mode = CacheMode::from_env_value(std::env::var("SPLAT_CACHE_MODE").ok().as_deref());
    open_store(&manifest_path, cache_dir, mode)
}

/// Open the store from explicit paths. Fails open: an absent or corrupt
/// manifest, or an absent cache directory, serves an empty listing and never
/// stops the server booting.
pub fn open_store(manifest_path: &str, cache_dir: PathBuf, mode: CacheMode) -> SplatStore {
    let manifest = match std::fs::read_to_string(&manifest_path)
        .map_err(|e| e.to_string())
        .and_then(|t| Manifest::from_json(&t).map_err(|e| e.to_string()))
    {
        Ok(m) => m,
        Err(e) => {
            log::warn!("splats: manifest {manifest_path} unusable ({e}); serving none");
            Manifest {
                version: visionclaw_splat::manifest::MANIFEST_VERSION,
                assets: vec![],
            }
        }
    };
    let (store, refusals) = SplatStore::open(manifest, cache_dir, mode);
    for r in &refusals {
        log::warn!("splats: refused: {r}");
    }
    let served: usize = store
        .listing()
        .assets
        .iter()
        .map(|a| a.variants.len())
        .sum();
    log::info!(
        "splats: {served} variant(s) of {} asset(s) served from {} ({mode:?} cache)",
        store.listing().assets.len(),
        store.dir().display()
    );
    store
}

/// `GET /api/splats`
pub async fn list_splats(store: web::Data<SplatStore>) -> impl Responder {
    HttpResponse::Ok()
        .insert_header((header::CACHE_CONTROL, "no-cache"))
        .json(store.listing())
}

/// `GET /api/splats/blob/{address}`
pub async fn get_splat_blob(
    req: HttpRequest,
    path: web::Path<String>,
    store: web::Data<SplatStore>,
) -> actix_web::Result<HttpResponse> {
    let address = path.into_inner();
    let Some((file, _len)) = store.blob_path(&address) else {
        return Ok(HttpResponse::NotFound().finish());
    };
    let named = NamedFile::open_async(file)
        .await?
        .set_content_type(ContentType::octet_stream().0)
        .use_etag(false)
        .use_last_modified(false)
        .disable_content_disposition();
    let mut resp = named.into_response(&req);
    let h = resp.headers_mut();
    // Content-addressed: the bytes behind an address never change.
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    if let Ok(etag) = HeaderValue::from_str(&format!("\"{address}\"")) {
        h.insert(header::ETAG, etag);
    }
    Ok(resp)
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/splats")
            .route("", web::get().to(list_splats))
            .route("/blob/{address}", web::get().to(get_splat_blob)),
    );
}

#[cfg(test)]
mod tests {
    #[actix_web::test]
    async fn boot_fails_open_on_absent_or_corrupt_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let corrupt = dir.path().join("corrupt.json");
        std::fs::write(&corrupt, b"{not json").unwrap();
        let missing = dir.path().join("absent.json");
        for manifest in [missing.to_str().unwrap(), corrupt.to_str().unwrap()] {
            let store = super::open_store(
                manifest,
                dir.path().join("no-such-cache"),
                super::CacheMode::from_env_value(None),
            );
            assert!(store.listing().assets.is_empty());
        }
        // The committed manifest has only pending variants: it lists nothing.
        let committed = concat!(env!("CARGO_MANIFEST_DIR"), "/data/splats/manifest.json");
        let store = super::open_store(
            committed,
            dir.path().join("no-such-cache"),
            super::CacheMode::from_env_value(None),
        );
        assert!(store.listing().assets.iter().all(|a| a.variants.is_empty()));
    }

    use super::*;
    use actix_web::{test, App};
    use visionclaw_splat::manifest::{Bounds, Licence, SplatAsset, Variant, VariantName};
    use visionclaw_splat::spz::{encode_v3, Splats};
    use visionclaw_splat::store::admit_variant;

    fn fixture() -> (tempfile::TempDir, SplatStore, Vec<u8>, String) {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Splats::default();
        for i in 0..3 {
            s.positions.extend_from_slice(&[i as f32, 0.0, 0.0]);
            s.scales.extend_from_slice(&[0.1; 3]);
            s.rotations.extend_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            s.colors.extend_from_slice(&[0.5, 0.5, 0.5, 1.0]);
        }
        let bytes = encode_v3(&s, 12);
        let asset = SplatAsset {
            id: "t".into(),
            title: "T".into(),
            author: "A".into(),
            source_url: "https://example.invalid".into(),
            licence: Licence {
                spdx: "CC0-1.0".into(),
                url: "https://creativecommons.org/publicdomain/zero/1.0/".into(),
            },
            grant: None,
            attribution: "T by A".into(),
            upstream_unchecked: vec![],
            sources: vec![],
            bounds: Bounds {
                min: [-1.0; 3],
                max: [1.0; 3],
            },
            stage: None,
            variants: vec![Variant {
                name: VariantName::Xr1,
                splats: 3,
                sh_degree: 0,
                sha256: Some(visionclaw_splat::sha256_hex(&bytes)),
                bytes: Some(bytes.len() as u64),
            }],
        };
        let addr = admit_variant(
            &asset,
            &asset.variants[0],
            &bytes,
            dir.path(),
            CacheMode::Public,
        )
        .unwrap();
        let manifest = Manifest {
            version: 1,
            assets: vec![asset],
        };
        let (store, refusals) = SplatStore::open(manifest, dir.path(), CacheMode::Public);
        assert!(refusals.is_empty());
        (dir, store, bytes, addr)
    }

    #[actix_web::test]
    async fn the_listing_and_blob_are_served() {
        let (_dir, store, bytes, addr) = fixture();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(store))
                .service(web::scope("/api").configure(configure_routes)),
        )
        .await;

        let listing: serde_json::Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get().uri("/api/splats").to_request(),
        )
        .await;
        let v = &listing["assets"][0]["variants"][0];
        assert_eq!(v["name"], "xr-1");
        assert_eq!(v["address"], addr.as_str());
        assert_eq!(listing["assets"][0]["modified"], true);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(v["url"].as_str().unwrap())
                .to_request(),
        )
        .await;
        assert!(resp.status().is_success());
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            resp.headers().get(header::ETAG).unwrap().to_str().unwrap(),
            format!("\"{addr}\"")
        );
        assert_eq!(test::read_body(resp).await.as_ref(), &bytes[..]);
    }

    #[actix_web::test]
    async fn unknown_and_malformed_addresses_are_404() {
        let (_dir, store, _, _) = fixture();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(store))
                .service(web::scope("/api").configure(configure_routes)),
        )
        .await;
        for uri in [
            "/api/splats/blob/sha256-12-000000000000",
            "/api/splats/blob/..%2F..%2Fetc%2Fpasswd",
            "/api/splats/blob/SHA256-12-ABC",
        ] {
            let resp =
                test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
            assert_eq!(resp.status(), 404, "{uri}");
        }
    }
}
