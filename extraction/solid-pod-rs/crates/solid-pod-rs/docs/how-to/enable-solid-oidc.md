# How to enable Solid-OIDC

**Goal:** accept DPoP-bound access tokens alongside (or instead of)
NIP-98.

## Step 1 — Enable the feature

In your crate's `Cargo.toml`:

```toml
[dependencies]
solid-pod-rs = { version = "0.2", features = ["oidc"] }
```

This pulls in `openidconnect` and `jsonwebtoken` dependencies. The
feature is off by default to keep the dependency surface small for
NIP-98-only deployments.

## Step 2 — Serve a discovery document

At `GET /.well-known/openid-configuration` return:

```rust
use solid_pod_rs::oidc::discovery_for;

let doc = discovery_for("https://op.example.com");
HttpResponse::Ok()
    .content_type("application/json")
    .json(doc)
```

The structure includes the full
[DiscoveryDocument](../reference/api.md#oidc-types) set of endpoints
(`authorize`, `token`, `userinfo`, `jwks`, `register`, `introspect`),
the supported DPoP signing algorithms (`ES256`, `RS256`), and the
`solid_oidc_supported` claim.

## Step 3 — Register clients dynamically

```rust
use solid_pod_rs::oidc::{register_client, ClientRegistrationRequest};

async fn handle_register(req: web::Json<ClientRegistrationRequest>) -> HttpResponse {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let resp = register_client(&req, now);
    // Persist `resp` against `resp.client_id` in your store.
    HttpResponse::Created().json(resp)
}
```

See [reference/api.md §oidc](../reference/api.md#oidc-types) for the
full request/response shape (RFC 7591).

## Step 4 — Verify the DPoP proof and access token

On every authenticated request:

```rust
use solid_pod_rs::oidc::{verify_dpop_proof, verify_access_token};

// 1. DPoP proof binds this request to a keypair.
let dpop_header = req.headers().get("dpop").unwrap().to_str()?;
let expected_htu = format!("https://pod.example.com{}", req.uri().path());
let dpop = verify_dpop_proof(
    dpop_header,
    &expected_htu,
    req.method().as_str(),
    now,
    /* iat skew seconds */ 60,
)?;

// 2. Access token: bearer, cnf.jkt must equal dpop.jkt.
let bearer = req.headers().get("authorization")
    .and_then(|v| v.to_str().ok())
    .and_then(|v| v.strip_prefix("Bearer "))
    .ok_or(...)?;
let access = verify_access_token(
    bearer,
    /* HS256 secret, or see note */ &hmac_secret,
    "https://op.example.com",
    &dpop.jkt,
    now,
)?;

let webid = access.webid;
```

### Note on signing algorithms

`verify_access_token` uses HS256 in the current build so the test
vector path is deterministic. Production OPs sign access tokens with
ES256 or RS256. To support these, fetch the OP's JWKS and decode the
token with the appropriate
`jsonwebtoken::DecodingKey::from_{ec,rsa}_pem`. The public API will
evolve to accept a `DecodingKey` directly in a future release.

## Step 5 — Extract the WebID

`verify_access_token` returns `AccessTokenVerified { webid, ... }`.
`extract_webid` is exposed separately if you have already verified
the token elsewhere:

```rust
let webid = oidc::extract_webid(&claims)?;
```

The extractor prefers the explicit `webid` claim; falls back to `sub`
if it is URL-shaped. See [Solid-OIDC §5.4](https://solid.github.io/solid-oidc/#webid-integration).

## Step 6 — Introspection (optional)

Expose `POST /introspect` for clients that want to verify their own
tokens:

```rust
use solid_pod_rs::oidc::IntrospectionResponse;

let body = if let Ok(verified) = verify_access_token(...) {
    IntrospectionResponse::from_verified(&verified)
} else {
    IntrospectionResponse::inactive()
};
HttpResponse::Ok().json(body)
```

## See also

- [reference/api.md §oidc](../reference/api.md#oidc-types)
- [explanation/security-model.md §Solid-OIDC layer](../explanation/security-model.md)
- [RFC 9449 (DPoP)](https://datatracker.ietf.org/doc/html/rfc9449)
- [Solid-OIDC spec](https://solid.github.io/solid-oidc/)
