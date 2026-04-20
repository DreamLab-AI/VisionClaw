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
            .map(|d| evaluate_access(Some(d), None, path, AccessMode::Read))
            .unwrap_or(true),
        AccessMode::Write | AccessMode::Append | AccessMode::Control => acl_doc
            .map(|d| evaluate_access(Some(d), None, path, required))
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
