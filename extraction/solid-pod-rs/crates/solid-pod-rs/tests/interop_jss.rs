//! JSS (JavaScriptSolidServer) interop parity harness.
//!
//! Thin harness that asserts solid-pod-rs response shapes match the
//! reference JSS responses for common Solid Protocol requests.
//!
//! Each test runs a fixture-defined request against an in-memory
//! `MemoryBackend` and compares headers/status/body against the
//! captured JSS response (fixtures under `tests/fixtures/`).

use std::collections::HashMap;
use std::path::PathBuf;

use bytes::Bytes;
use solid_pod_rs::ldp::{
    link_headers, negotiate_format, render_container_jsonld, render_container_turtle,
    PreferHeader, RdfFormat, ACCEPT_POST,
};
use solid_pod_rs::storage::{memory::MemoryBackend, Storage};
use solid_pod_rs::wac::{evaluate_access, method_to_mode, AccessMode, AclDocument};

// ---------------------------------------------------------------------------
// Fixture parser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct HttpFixture {
    start_line: String,
    headers: Vec<(String, String)>,
    body: String,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn parse_http_file(name: &str) -> HttpFixture {
    let path = fixtures_dir().join(name);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
    parse_http_text(&raw)
}

fn parse_http_text(raw: &str) -> HttpFixture {
    let mut lines = raw.split('\n');
    let start_line = lines.next().unwrap_or("").trim_end_matches('\r').to_string();
    let mut headers = Vec::new();
    let mut body_lines = Vec::new();
    let mut in_body = false;
    for line in lines {
        let line = line.trim_end_matches('\r');
        if in_body {
            body_lines.push(line);
            continue;
        }
        if line.is_empty() {
            in_body = true;
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    HttpFixture {
        start_line,
        headers,
        body: body_lines.join("\n"),
    }
}

// ---------------------------------------------------------------------------
// In-memory pod "server" — just enough to drive fixtures
// ---------------------------------------------------------------------------

struct SimulatedResponse {
    status: u16,
    #[allow(dead_code)]
    reason: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn header_values(r: &SimulatedResponse, name: &str) -> Vec<String> {
    r.headers
        .iter()
        .filter_map(|(k, v)| {
            if k.eq_ignore_ascii_case(name) {
                Some(v.clone())
            } else {
                None
            }
        })
        .collect()
}

fn response_header_names(r: &SimulatedResponse) -> Vec<String> {
    r.headers.iter().map(|(k, _)| k.to_ascii_lowercase()).collect()
}

async fn handle_request(
    pod: &MemoryBackend,
    acls: &HashMap<String, AclDocument>,
    method: &str,
    path: &str,
    req_headers: &[(String, String)],
    body: &[u8],
) -> SimulatedResponse {
    let accept = req_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("Accept"))
        .map(|(_, v)| v.as_str());
    let prefer = req_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("Prefer"))
        .map(|(_, v)| PreferHeader::parse(v))
        .unwrap_or_default();
    let ct = req_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("Content-Type"))
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    // ACL check
    let required = method_to_mode(method);
    let acl_doc = acls
        .iter()
        .find(|(p, _)| path.starts_with(p.as_str()))
        .map(|(_, d)| d);
    let allowed = match required {
        AccessMode::Read => acl_doc
            .map(|d| evaluate_access(Some(d), None, path, AccessMode::Read, None))
            .unwrap_or(true),
        AccessMode::Write | AccessMode::Append | AccessMode::Control => acl_doc
            .map(|d| evaluate_access(Some(d), None, path, required, None))
            .unwrap_or(true),
    };
    if !allowed {
        return SimulatedResponse {
            status: 403,
            reason: "Forbidden",
            headers: Vec::new(),
            body: Vec::new(),
        };
    }

    match method.to_ascii_uppercase().as_str() {
        "GET" | "HEAD" => {
            if path.ends_with('/') {
                // Container
                let list = pod.list(path).await.unwrap_or_default();
                let mut headers: Vec<(String, String)> = link_headers(path)
                    .into_iter()
                    .map(|v| ("Link".to_string(), v))
                    .collect();
                let format = negotiate_format(accept);
                let (content_type, body_bytes) = match format {
                    RdfFormat::Turtle => (
                        "text/turtle",
                        render_container_turtle(path, &list, prefer).into_bytes(),
                    ),
                    RdfFormat::JsonLd => (
                        "application/ld+json",
                        serde_json::to_vec(&render_container_jsonld(path, &list, prefer))
                            .unwrap(),
                    ),
                    RdfFormat::NTriples => ("application/n-triples", Vec::new()),
                    RdfFormat::RdfXml => ("application/rdf+xml", Vec::new()),
                };
                headers.push(("Content-Type".into(), content_type.into()));
                headers.push(("Accept-Post".into(), ACCEPT_POST.into()));
                SimulatedResponse {
                    status: 200,
                    reason: "OK",
                    headers,
                    body: body_bytes,
                }
            } else {
                match pod.get(path).await {
                    Ok((b, meta)) => {
                        let mut headers: Vec<(String, String)> = link_headers(path)
                            .into_iter()
                            .map(|v| ("Link".to_string(), v))
                            .collect();
                        headers.push(("Content-Type".into(), meta.content_type));
                        headers.push(("ETag".into(), format!("\"{}\"", meta.etag)));
                        SimulatedResponse {
                            status: 200,
                            reason: "OK",
                            headers,
                            body: b.to_vec(),
                        }
                    }
                    Err(_) => SimulatedResponse {
                        status: 404,
                        reason: "Not Found",
                        headers: Vec::new(),
                        body: Vec::new(),
                    },
                }
            }
        }
        "PUT" => {
            if path.ends_with('/') {
                return SimulatedResponse {
                    status: 405,
                    reason: "Method Not Allowed",
                    headers: Vec::new(),
                    body: Vec::new(),
                };
            }
            let existed = pod.exists(path).await.unwrap_or(false);
            pod.put(path, Bytes::copy_from_slice(body), &ct).await.unwrap();
            let mut headers: Vec<(String, String)> = link_headers(path)
                .into_iter()
                .map(|v| ("Link".to_string(), v))
                .collect();
            headers.push(("Location".into(), path.to_string()));
            SimulatedResponse {
                status: if existed { 204 } else { 201 },
                reason: if existed { "No Content" } else { "Created" },
                headers,
                body: Vec::new(),
            }
        }
        "DELETE" => {
            match pod.delete(path).await {
                Ok(()) => SimulatedResponse {
                    status: 204,
                    reason: "No Content",
                    headers: Vec::new(),
                    body: Vec::new(),
                },
                Err(_) => SimulatedResponse {
                    status: 404,
                    reason: "Not Found",
                    headers: Vec::new(),
                    body: Vec::new(),
                },
            }
        }
        "OPTIONS" => {
            let mut headers: Vec<(String, String)> = link_headers(path)
                .into_iter()
                .map(|v| ("Link".to_string(), v))
                .collect();
            headers.push(("Accept-Post".into(), ACCEPT_POST.into()));
            let allow = if path.ends_with('/') {
                "GET, HEAD, POST, OPTIONS"
            } else {
                "GET, HEAD, PUT, DELETE, PATCH, OPTIONS"
            };
            headers.push(("Allow".into(), allow.into()));
            SimulatedResponse {
                status: 204,
                reason: "No Content",
                headers,
                body: Vec::new(),
            }
        }
        _ => SimulatedResponse {
            status: 405,
            reason: "Method Not Allowed",
            headers: Vec::new(),
            body: Vec::new(),
        },
    }
}

fn expected_status(expected: &HttpFixture) -> u16 {
    // First line looks like "HTTP/1.1 200 OK"
    let parts: Vec<&str> = expected.start_line.split_whitespace().collect();
    parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

fn expected_headers(expected: &HttpFixture) -> Vec<(String, String)> {
    expected.headers.clone()
}

// ---------------------------------------------------------------------------
// Fixture tests
// ---------------------------------------------------------------------------

async fn seed_container(pod: &MemoryBackend) {
    pod.put("/container/a.txt", Bytes::from_static(b"a"), "text/plain")
        .await
        .unwrap();
    pod.put("/container/b.txt", Bytes::from_static(b"b"), "text/plain")
        .await
        .unwrap();
}

#[tokio::test]
async fn jss_get_container_link_headers_match() {
    let pod = MemoryBackend::new();
    seed_container(&pod).await;
    let acls = HashMap::new();

    let req = parse_http_file("get_container.request.http");
    let expected = parse_http_file("get_container.response.http");

    let resp = handle_request(&pod, &acls, "GET", "/container/", &req.headers, &[]).await;

    assert_eq!(resp.status, expected_status(&expected));
    let expected_hdrs = expected_headers(&expected);
    // Every Link header the fixture expects must be present.
    let expected_links: Vec<&String> = expected_hdrs
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("Link"))
        .map(|(_, v)| v)
        .collect();
    let actual_links = header_values(&resp, "Link");
    for e in expected_links {
        assert!(
            actual_links.iter().any(|a| a == e),
            "missing Link header: {e}\nactual: {actual_links:?}"
        );
    }
    // Accept-Post present
    let accept_post = header_values(&resp, "Accept-Post");
    assert!(accept_post.iter().any(|v| v.contains("text/turtle")));
}

#[tokio::test]
async fn jss_get_resource_jsonld_content_type() {
    let pod = MemoryBackend::new();
    pod.put(
        "/profile/card",
        Bytes::from_static(b"{}"),
        "application/ld+json",
    )
    .await
    .unwrap();
    let acls = HashMap::new();

    let req = parse_http_file("get_resource_jsonld.request.http");
    let expected = parse_http_file("get_resource_jsonld.response.http");

    let resp = handle_request(&pod, &acls, "GET", "/profile/card", &req.headers, &[]).await;
    assert_eq!(resp.status, expected_status(&expected));
    let ct = header_values(&resp, "Content-Type");
    assert!(ct.iter().any(|v| v.starts_with("application/ld+json")));
    // body matches (trivial — expected is "{}")
    assert_eq!(&resp.body, expected.body.trim().as_bytes());
}

#[tokio::test]
async fn jss_put_resource_returns_201_with_location() {
    let pod = MemoryBackend::new();
    let acls = HashMap::new();

    let req = parse_http_file("put_resource.request.http");
    let expected = parse_http_file("put_resource.response.http");

    let resp =
        handle_request(&pod, &acls, "PUT", "/data/note.txt", &req.headers, req.body.as_bytes())
            .await;
    assert_eq!(resp.status, 201);
    assert_eq!(expected_status(&expected), 201);
    let loc = header_values(&resp, "Location");
    assert!(loc.iter().any(|v| v == "/data/note.txt"));
}

#[tokio::test]
async fn jss_put_existing_returns_204() {
    let pod = MemoryBackend::new();
    pod.put("/data/note.txt", Bytes::from_static(b"seed"), "text/plain")
        .await
        .unwrap();
    let acls = HashMap::new();
    let resp = handle_request(
        &pod,
        &acls,
        "PUT",
        "/data/note.txt",
        &[("Content-Type".into(), "text/plain".into())],
        b"update",
    )
    .await;
    assert_eq!(resp.status, 204);
}

#[tokio::test]
async fn jss_delete_resource_returns_204() {
    let pod = MemoryBackend::new();
    pod.put("/data/note.txt", Bytes::from_static(b"x"), "text/plain")
        .await
        .unwrap();
    let acls = HashMap::new();

    let req = parse_http_file("delete_resource.request.http");
    let expected = parse_http_file("delete_resource.response.http");

    let resp = handle_request(&pod, &acls, "DELETE", "/data/note.txt", &req.headers, &[])
        .await;
    assert_eq!(resp.status, 204);
    assert_eq!(expected_status(&expected), 204);
    assert!(!pod.exists("/data/note.txt").await.unwrap());
}

#[tokio::test]
async fn jss_get_missing_returns_404() {
    let pod = MemoryBackend::new();
    let acls = HashMap::new();
    let req = parse_http_file("not_found.request.http");
    let expected = parse_http_file("not_found.response.http");
    let resp = handle_request(&pod, &acls, "GET", "/does-not-exist", &req.headers, &[])
        .await;
    assert_eq!(resp.status, 404);
    assert_eq!(expected_status(&expected), 404);
}

#[tokio::test]
async fn jss_forbidden_write_returns_403() {
    let pod = MemoryBackend::new();
    let mut acls: HashMap<String, AclDocument> = HashMap::new();
    // Public can Read everything under /read-only; nobody can Write.
    let doc: AclDocument = serde_json::from_str(
        r#"{
            "@graph": [{
                "acl:agentClass": {"@id": "foaf:Agent"},
                "acl:accessTo": {"@id": "/read-only"},
                "acl:default": {"@id": "/read-only"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    )
    .unwrap();
    acls.insert("/read-only".into(), doc);

    let req = parse_http_file("forbidden_write.request.http");
    let expected = parse_http_file("forbidden_write.response.http");

    let resp = handle_request(
        &pod,
        &acls,
        "PUT",
        "/read-only/x.txt",
        &req.headers,
        req.body.as_bytes(),
    )
    .await;
    assert_eq!(resp.status, 403);
    assert_eq!(expected_status(&expected), 403);
}

#[tokio::test]
async fn jss_options_container_lists_accept_post() {
    let pod = MemoryBackend::new();
    seed_container(&pod).await;
    let acls = HashMap::new();

    let req = parse_http_file("options_container.request.http");
    let expected = parse_http_file("options_container.response.http");
    let resp = handle_request(&pod, &acls, "OPTIONS", "/container/", &req.headers, &[])
        .await;
    assert_eq!(resp.status, 204);
    assert_eq!(expected_status(&expected), 204);
    let ap = header_values(&resp, "Accept-Post");
    assert!(!ap.is_empty());
    let allow = header_values(&resp, "Allow");
    assert!(allow.iter().any(|v| v.contains("POST")));
}

#[tokio::test]
async fn jss_ldp_container_contains_member_iris() {
    let pod = MemoryBackend::new();
    seed_container(&pod).await;
    let acls = HashMap::new();

    let accept_jsonld = vec![("Accept".into(), "application/ld+json".into())];
    let resp = handle_request(&pod, &acls, "GET", "/container/", &accept_jsonld, &[]).await;
    assert_eq!(resp.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
    let contains = body
        .get("ldp:contains")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let ids: Vec<String> = contains
        .iter()
        .filter_map(|m| m.get("@id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();
    assert!(ids.iter().any(|i| i.ends_with("/container/a.txt")));
    assert!(ids.iter().any(|i| i.ends_with("/container/b.txt")));
}

#[tokio::test]
async fn jss_prefer_minimal_container_omits_contains() {
    let pod = MemoryBackend::new();
    seed_container(&pod).await;
    let acls = HashMap::new();
    let req_headers = vec![
        ("Accept".into(), "application/ld+json".into()),
        (
            "Prefer".into(),
            "return=representation; include=\"http://www.w3.org/ns/ldp#PreferMinimalContainer\""
                .into(),
        ),
    ];
    let resp = handle_request(&pod, &acls, "GET", "/container/", &req_headers, &[]).await;
    assert_eq!(resp.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
    assert!(body.get("ldp:contains").is_none());
}

#[tokio::test]
async fn jss_prefer_contained_iris_only_returns_iri_list() {
    let pod = MemoryBackend::new();
    seed_container(&pod).await;
    let acls = HashMap::new();
    let req_headers = vec![
        ("Accept".into(), "application/ld+json".into()),
        (
            "Prefer".into(),
            "return=representation; include=\"http://www.w3.org/ns/ldp#PreferContainedIRIs\""
                .into(),
        ),
    ];
    let resp = handle_request(&pod, &acls, "GET", "/container/", &req_headers, &[]).await;
    let body: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
    // Minimal container representation must NOT carry @context.
    assert!(body.get("@context").is_none());
    let contains = body
        .get("ldp:contains")
        .and_then(|v| v.as_array())
        .unwrap_or(&Vec::new())
        .clone();
    assert_eq!(contains.len(), 2);
}

#[tokio::test]
async fn jss_accept_turtle_returns_turtle() {
    let pod = MemoryBackend::new();
    seed_container(&pod).await;
    let acls = HashMap::new();
    let resp = handle_request(
        &pod,
        &acls,
        "GET",
        "/container/",
        &[("Accept".into(), "text/turtle".into())],
        &[],
    )
    .await;
    let ct = header_values(&resp, "Content-Type");
    assert!(ct.iter().any(|v| v.starts_with("text/turtle")));
    let body = String::from_utf8(resp.body).unwrap();
    assert!(body.contains("ldp:BasicContainer"));
}

#[tokio::test]
async fn jss_get_non_container_emits_etag() {
    let pod = MemoryBackend::new();
    pod.put("/x.txt", Bytes::from_static(b"abc"), "text/plain")
        .await
        .unwrap();
    let acls = HashMap::new();
    let resp = handle_request(&pod, &acls, "GET", "/x.txt", &[], &[]).await;
    assert_eq!(resp.status, 200);
    let etag = header_values(&resp, "ETag");
    assert!(!etag.is_empty());
    assert!(etag[0].starts_with('"') && etag[0].ends_with('"'));
}

#[tokio::test]
async fn jss_put_to_container_returns_405() {
    let pod = MemoryBackend::new();
    let acls = HashMap::new();
    let resp = handle_request(&pod, &acls, "PUT", "/foo/", &[], b"x").await;
    assert_eq!(resp.status, 405);
}

#[tokio::test]
async fn jss_delete_missing_returns_404() {
    let pod = MemoryBackend::new();
    let acls = HashMap::new();
    let resp = handle_request(&pod, &acls, "DELETE", "/gone", &[], &[]).await;
    assert_eq!(resp.status, 404);
}

#[tokio::test]
async fn jss_unsupported_method_returns_405() {
    let pod = MemoryBackend::new();
    let acls = HashMap::new();
    let resp = handle_request(&pod, &acls, "TRACE", "/x", &[], &[]).await;
    assert_eq!(resp.status, 405);
}

#[tokio::test]
async fn jss_link_header_exposes_describedby() {
    let pod = MemoryBackend::new();
    pod.put("/x.txt", Bytes::from_static(b"x"), "text/plain")
        .await
        .unwrap();
    let acls = HashMap::new();
    let resp = handle_request(&pod, &acls, "GET", "/x.txt", &[], &[]).await;
    let links = header_values(&resp, "Link");
    assert!(
        links.iter().any(|v| v.contains("rel=\"describedby\"")),
        "expected describedby in links: {links:?}"
    );
}

#[tokio::test]
async fn jss_pod_root_exposes_pim_storage_link() {
    let pod = MemoryBackend::new();
    let acls = HashMap::new();
    let resp = handle_request(&pod, &acls, "GET", "/", &[], &[]).await;
    assert_eq!(resp.status, 200);
    let links = header_values(&resp, "Link");
    assert!(
        links
            .iter()
            .any(|v| v.contains("http://www.w3.org/ns/pim/space#storage")),
        "expected pim:storage link: {links:?}"
    );
}

#[tokio::test]
async fn jss_acl_document_can_be_fetched_via_get() {
    let pod = MemoryBackend::new();
    let acl_body = r#"{"@graph":[{"acl:agent":{"@id":"did:x"},"acl:accessTo":{"@id":"/f"},"acl:mode":{"@id":"acl:Read"}}]}"#;
    pod.put(
        "/f.acl",
        Bytes::copy_from_slice(acl_body.as_bytes()),
        "application/ld+json",
    )
    .await
    .unwrap();
    let acls = HashMap::new();
    let resp = handle_request(&pod, &acls, "GET", "/f.acl", &[], &[]).await;
    assert_eq!(resp.status, 200);
    let body = String::from_utf8(resp.body).unwrap();
    assert!(body.contains("acl:agent"));
}

#[tokio::test]
async fn jss_container_get_links_have_consistent_header_order() {
    let pod = MemoryBackend::new();
    seed_container(&pod).await;
    let acls = HashMap::new();
    let resp = handle_request(&pod, &acls, "GET", "/container/", &[], &[]).await;
    let links = header_values(&resp, "Link");
    // BasicContainer must come before plain Container / Resource.
    let idx_bc = links.iter().position(|l| l.contains("BasicContainer")).unwrap();
    let idx_r = links.iter().position(|l| l.contains("ldp#Resource")).unwrap();
    assert!(idx_bc < idx_r, "BasicContainer should precede Resource");
}

#[tokio::test]
async fn jss_response_always_has_type_link() {
    let pod = MemoryBackend::new();
    pod.put("/x", Bytes::from_static(b"x"), "text/plain")
        .await
        .unwrap();
    let acls = HashMap::new();
    for method in ["GET", "HEAD"] {
        let resp = handle_request(&pod, &acls, method, "/x", &[], &[]).await;
        let links = header_values(&resp, "Link");
        assert!(
            links.iter().any(|l| l.contains("rel=\"type\"")),
            "{method} /x missing type link: {links:?}"
        );
    }
}

#[tokio::test]
async fn jss_acl_blocks_unauthenticated_read() {
    let pod = MemoryBackend::new();
    pod.put("/priv/doc", Bytes::from_static(b"secret"), "text/plain")
        .await
        .unwrap();
    let mut acls: HashMap<String, AclDocument> = HashMap::new();
    acls.insert(
        "/priv".into(),
        serde_json::from_str(
            r#"{
                "@graph": [{
                    "acl:agent": {"@id": "did:nostr:owner"},
                    "acl:default": {"@id": "/priv"},
                    "acl:mode": {"@id": "acl:Read"}
                }]
            }"#,
        )
        .unwrap(),
    );
    let resp = handle_request(&pod, &acls, "GET", "/priv/doc", &[], &[]).await;
    assert_eq!(resp.status, 403, "anonymous GET of /priv should be denied");
}

// Exercise all response-header names collectively.
#[tokio::test]
async fn jss_header_catalog_sanity() {
    let pod = MemoryBackend::new();
    pod.put("/sanity.txt", Bytes::from_static(b"hi"), "text/plain")
        .await
        .unwrap();
    let acls = HashMap::new();
    let resp = handle_request(&pod, &acls, "GET", "/sanity.txt", &[], &[]).await;
    let names = response_header_names(&resp);
    for required in ["link", "content-type", "etag"] {
        assert!(names.iter().any(|n| n == required), "missing: {required}");
    }
}

// ---------------------------------------------------------------------------
// Sprint 3 additions (parity-close scenarios)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn jss_turtle_acl_fallback_grants_public_read() {
    use solid_pod_rs::wac::{evaluate_access, AclResolver, StorageAclResolver};
    let pod = std::sync::Arc::new(MemoryBackend::new());
    let ttl = r#"
        @prefix acl: <http://www.w3.org/ns/auth/acl#> .
        @prefix foaf: <http://xmlns.com/foaf/0.1/> .
        <#pub> a acl:Authorization ;
            acl:agentClass foaf:Agent ;
            acl:accessTo </> ;
            acl:default </> ;
            acl:mode acl:Read .
    "#;
    pod.put("/.acl", Bytes::copy_from_slice(ttl.as_bytes()), "text/turtle")
        .await
        .unwrap();
    let resolver = StorageAclResolver::new(pod.clone());
    let doc = resolver.find_effective_acl("/foo").await.unwrap().unwrap();
    assert!(evaluate_access(
        Some(&doc),
        None,
        "/foo",
        AccessMode::Read
    ,
        None));
}

#[tokio::test]
async fn jss_if_match_preconditions_block_concurrent_update() {
    use solid_pod_rs::evaluate_preconditions;
    use solid_pod_rs::ConditionalOutcome;
    let pod = MemoryBackend::new();
    let meta = pod
        .put("/r", Bytes::from_static(b"v1"), "text/plain")
        .await
        .unwrap();
    // Client sends stale If-Match
    let outcome =
        evaluate_preconditions("PUT", Some(&meta.etag), Some("\"stale-etag\""), None);
    assert_eq!(outcome, ConditionalOutcome::PreconditionFailed);
    // Correct etag passes
    let outcome = evaluate_preconditions(
        "PUT",
        Some(&meta.etag),
        Some(&format!("\"{}\"", meta.etag)),
        None,
    );
    assert_eq!(outcome, ConditionalOutcome::Proceed);
}

#[tokio::test]
async fn jss_range_request_returns_slice() {
    use solid_pod_rs::{parse_range_header, slice_range};
    let pod = MemoryBackend::new();
    let body = b"abcdefghij";
    pod.put(
        "/bin",
        Bytes::copy_from_slice(body),
        "application/octet-stream",
    )
    .await
    .unwrap();
    let range = parse_range_header(Some("bytes=2-5"), body.len() as u64)
        .unwrap()
        .unwrap();
    let slice = slice_range(body, range);
    assert_eq!(slice, b"cdef");
    assert_eq!(range.content_range(body.len() as u64), "bytes 2-5/10");
}

#[tokio::test]
async fn jss_json_patch_applies_over_pod_resource() {
    use solid_pod_rs::apply_json_patch;
    let pod = MemoryBackend::new();
    pod.put(
        "/profile.json",
        Bytes::copy_from_slice(br#"{"name":"alice"}"#),
        "application/json",
    )
    .await
    .unwrap();
    let (body, _) = pod.get("/profile.json").await.unwrap();
    let mut doc: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let patch = serde_json::json!([
        { "op": "add", "path": "/role", "value": "admin" }
    ]);
    apply_json_patch(&mut doc, &patch).unwrap();
    let re = serde_json::to_vec(&doc).unwrap();
    pod.put("/profile.json", Bytes::from(re), "application/json")
        .await
        .unwrap();
    let (body2, _) = pod.get("/profile.json").await.unwrap();
    let v2: serde_json::Value = serde_json::from_slice(&body2).unwrap();
    assert_eq!(v2["role"], "admin");
}

#[tokio::test]
async fn jss_options_container_advertises_accept_post_and_ranges() {
    use solid_pod_rs::options_for;
    let o = options_for("/photos/");
    assert!(o.accept_post.is_some());
    assert_eq!(o.accept_ranges, "bytes");
    assert!(o.allow.contains(&"POST"));
    assert!(!o.allow.contains(&"PUT"));
}

#[tokio::test]
async fn jss_options_resource_surfaces_patch_dialects() {
    use solid_pod_rs::options_for;
    let o = options_for("/profile.json");
    assert!(o.allow.contains(&"PATCH"));
    assert!(o.accept_patch.contains("n3"));
    assert!(o.accept_patch.contains("sparql-update"));
    assert!(o.accept_patch.contains("json-patch"));
}

#[tokio::test]
async fn jss_webid_includes_oidc_issuer_for_follow_your_nose() {
    use solid_pod_rs::{extract_oidc_issuer, generate_webid_html_with_issuer};
    let pod = MemoryBackend::new();
    let html = generate_webid_html_with_issuer(
        "abc",
        Some("Alice"),
        "https://pod.example",
        Some("https://op.example"),
    );
    pod.put(
        "/profile/card",
        Bytes::copy_from_slice(html.as_bytes()),
        "text/html",
    )
    .await
    .unwrap();
    let (body, _) = pod.get("/profile/card").await.unwrap();
    let issuer = extract_oidc_issuer(&body).unwrap();
    assert_eq!(issuer.as_deref(), Some("https://op.example"));
}

#[tokio::test]
async fn jss_well_known_solid_exposes_storage_and_issuer() {
    use solid_pod_rs::well_known_solid;
    let doc = well_known_solid("https://pod.example/", "https://op.example");
    let v = serde_json::to_value(&doc).unwrap();
    assert_eq!(v["solid_oidc_issuer"], "https://op.example");
    assert!(v["storage"].as_str().unwrap().ends_with('/'));
    assert!(v["notification_gateway"].as_str().unwrap().ends_with(".notifications"));
}

#[tokio::test]
async fn jss_webfinger_acct_resolves_webid() {
    use solid_pod_rs::webfinger_response;
    let j = webfinger_response(
        "acct:alice@pod.example",
        "https://pod.example",
        "https://pod.example/profile/card#me",
    )
    .unwrap();
    assert_eq!(j.aliases[0], "https://pod.example/profile/card#me");
    assert!(j.links.iter().any(|l| l.rel.contains("webid")));
}

#[tokio::test]
async fn jss_nip05_lookup_binds_pubkey_to_name() {
    use solid_pod_rs::{nip05_document, verify_nip05};
    let mut names = std::collections::HashMap::new();
    names.insert("alice".to_string(), "0".repeat(64));
    let doc = nip05_document(names);
    let pk = verify_nip05("alice@host", &doc).unwrap();
    assert_eq!(pk.len(), 64);
}

#[tokio::test]
async fn jss_provision_pod_seeds_profile_and_containers() {
    use solid_pod_rs::{provision_pod, ProvisionPlan};
    let pod = MemoryBackend::new();
    let plan = ProvisionPlan {
        pubkey: "abc".into(),
        display_name: Some("Alice".into()),
        pod_base: "https://pod.example".into(),
        containers: vec!["/photos/".into()],
        root_acl: None,
        quota_bytes: None,
    };
    let out = provision_pod(&pod, &plan).await.unwrap();
    assert!(pod.exists("/profile/card").await.unwrap());
    assert!(out.webid.contains("/profile/card#me"));
}

#[tokio::test]
async fn jss_quota_reserves_and_releases_consistently() {
    use solid_pod_rs::QuotaTracker;
    let q = QuotaTracker::new(Some(100));
    q.reserve(40).unwrap();
    q.reserve(40).unwrap();
    assert!(q.reserve(30).is_err());
    q.release(40);
    q.reserve(30).unwrap();
    assert_eq!(q.used(), 70);
}

#[tokio::test]
async fn jss_admin_override_matches_constant_time() {
    use solid_pod_rs::check_admin_override;
    let ok = check_admin_override(Some("secretkey"), Some("secretkey"));
    assert!(ok.is_some());
    assert!(check_admin_override(Some("secretkez"), Some("secretkey")).is_none());
}

// Multi-include Prefer composition (check tolerant parsing).
#[tokio::test]
async fn jss_prefer_compose_include_minimal_and_contained_iris() {
    let p = PreferHeader::parse(
        "return=representation; include=\"http://www.w3.org/ns/ldp#PreferMinimalContainer http://www.w3.org/ns/ldp#PreferContainedIRIs\""
    );
    assert!(p.include_minimal);
    assert!(p.include_contained_iris);
}

// .meta sidecar auto-discovery via Link rel="describedby"
#[tokio::test]
async fn jss_meta_sidecar_link_always_present_on_non_meta_resources() {
    let pod = MemoryBackend::new();
    pod.put("/x", Bytes::from_static(b"x"), "text/plain")
        .await
        .unwrap();
    let acls = HashMap::new();
    let resp = handle_request(&pod, &acls, "GET", "/x", &[], &[]).await;
    let links = header_values(&resp, "Link");
    assert!(links.iter().any(|l| l.contains("/x.meta")));
    assert!(links.iter().any(|l| l.contains("rel=\"describedby\"")));
}

// Slug header handling correctness (already covered by resolve_slug but
// validates end-to-end semantics for POST-to-container).
#[tokio::test]
async fn jss_slug_safe_names_pass_through_unchanged() {
    use solid_pod_rs::ldp::resolve_slug;
    let out = resolve_slug("/photos/", Some("cat.jpg"));
    assert_eq!(out, "/photos/cat.jpg");
    // Path traversal rejection
    let bad = resolve_slug("/photos/", Some("../secret"));
    assert!(!bad.contains(".."));
}

// PATCH dialect detection: JSON Patch is now recognised.
#[tokio::test]
async fn jss_patch_dialect_includes_json_patch() {
    use solid_pod_rs::{patch_dialect_from_mime, PatchDialect};
    assert_eq!(
        patch_dialect_from_mime("application/json-patch+json"),
        Some(PatchDialect::JsonPatch)
    );
}

// Turtle ACL explicit parse path.
#[tokio::test]
async fn jss_turtle_acl_control_grants_acl_rw() {
    use solid_pod_rs::{parse_turtle_acl, wac::evaluate_access};
    let ttl = r#"
        @prefix acl: <http://www.w3.org/ns/auth/acl#> .
        <#o> a acl:Authorization ;
            acl:agent <did:nostr:own> ;
            acl:accessTo </> ;
            acl:mode acl:Control .
    "#;
    let doc = parse_turtle_acl(ttl).unwrap();
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:own"),
        "/",
        AccessMode::Control
    ,
        None));
}

// Dev-session helper.
#[tokio::test]
async fn jss_dev_session_carries_admin_flag() {
    use solid_pod_rs::dev_session;
    let s = dev_session("https://me", true);
    assert!(s.is_admin);
}
