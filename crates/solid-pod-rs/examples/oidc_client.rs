//! Solid-OIDC client — end-to-end DPoP-bound access-token flow.
//!
//! This example demonstrates, hermetically, the four stages of
//! Solid-OIDC 0.1 that a client plus a verifying pod go through:
//!
//! 1. **Discovery** — fetch `/.well-known/openid-configuration`.
//! 2. **Dynamic registration** — POST a client metadata document to
//!    `registration_endpoint` and get back a `client_id`.
//! 3. **DPoP proof** — build a signed `dpop+jwt` with `htu`, `htm`,
//!    `iat`, and the client's public `jwk` header.
//! 4. **Verify** — the pod's `oidc::verify_access_token` checks the
//!    token's `cnf.jkt` against the DPoP thumbprint.
//!
//! Run with (feature-gated):
//! ```bash
//! cargo run --example oidc_client -p solid-pod-rs --features oidc
//! ```
//!
//! Expected output:
//! ```text
//! [discovery] issuer=https://op.example token=https://op.example/token
//! [register]  client_id=client-<uuid>
//! [dpop]      jkt=<base64url sha256 thumbprint>
//! [verify]    webid=https://me.example/profile#me client=client-123
//! ```
//!
//! Note: this example uses HS256 throughout so it needs no external
//! keys or network calls. A real deployment swaps HS256 for ES256 or
//! RS256 and pulls the JWKS from the discovery document's `jwks_uri`.
//! The DPoP flow (build proof, extract `jkt`, bind to `cnf`) is
//! identical.

#[cfg(feature = "oidc")]
mod run {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL;
    use base64::Engine;
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use sha2::{Digest, Sha256};
    use solid_pod_rs::oidc::{
        discovery_for, register_client, verify_access_token, verify_dpop_proof,
        ClientRegistrationRequest, CnfClaim, Jwk, SolidOidcClaims,
    };

    pub fn main() -> Result<(), Box<dyn std::error::Error>> {
        // --------------------------------------------------------------
        // 1. Discovery
        // --------------------------------------------------------------
        let issuer = "https://op.example";
        let discovery = discovery_for(issuer);
        println!(
            "[discovery] issuer={} token={}",
            discovery.issuer, discovery.token_endpoint
        );

        // --------------------------------------------------------------
        // 2. Dynamic client registration (RFC 7591)
        // --------------------------------------------------------------
        let reg_req = ClientRegistrationRequest {
            redirect_uris: vec!["https://app.example/cb".into()],
            client_name: Some("solid-pod-rs example".into()),
            client_uri: Some("https://app.example".into()),
            grant_types: vec!["authorization_code".into()],
            response_types: vec!["code".into()],
            scope: Some("openid webid offline_access".into()),
            token_endpoint_auth_method: Some("none".into()),
            application_type: Some("web".into()),
        };
        let now = 1_700_000_000u64;
        let reg_resp = register_client(&reg_req, now);
        println!("[register]  client_id={}", reg_resp.client_id);

        // --------------------------------------------------------------
        // 3. DPoP proof — demo uses HS256 + kty=oct; production uses
        //    ES256 + kty=EC with a real public/private key pair.
        // --------------------------------------------------------------
        let dpop_secret = b"dpop-demo-secret";
        let jwk = Jwk {
            kty: "oct".into(),
            alg: Some("HS256".into()),
            kid: None,
            use_: None,
            crv: None,
            x: None,
            y: None,
            n: None,
            e: None,
            k: Some(BASE64_URL.encode(dpop_secret)),
        };
        let jkt = jwk.thumbprint()?;
        println!("[dpop]      jkt={jkt}");

        let dpop_proof = build_dpop_proof(
            dpop_secret,
            &jwk,
            "https://pod.example/resource",
            "GET",
            now,
        )?;
        // F5 (Sprint 4): when the `dpop-replay-cache` feature is
        // enabled `verify_dpop_proof` is `async` and takes an optional
        // replay cache. The example passes `None` to preserve the
        // pre-F5 demo flow. When the feature is off, the sync form
        // is used directly.
        #[cfg(feature = "dpop-replay-cache")]
        let verified_dpop = {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(verify_dpop_proof(
                &dpop_proof,
                "https://pod.example/resource",
                "GET",
                now,
                60,
                None,
            ))?
        };
        #[cfg(not(feature = "dpop-replay-cache"))]
        let verified_dpop = verify_dpop_proof(
            &dpop_proof,
            "https://pod.example/resource",
            "GET",
            now,
            60,
        )?;
        assert_eq!(verified_dpop.jkt, jkt);

        // --------------------------------------------------------------
        // 4. Issue + verify a DPoP-bound access token
        // --------------------------------------------------------------
        let at_secret = b"at-demo-secret";
        let at = issue_access_token(at_secret, issuer, &jkt, now + 3600, now)?;
        let verified = verify_access_token(
            &at,
            at_secret,
            issuer,
            &verified_dpop.jkt,
            now,
        )?;
        println!(
            "[verify]    webid={} client={}",
            verified.webid,
            verified.client_id.as_deref().unwrap_or("<none>"),
        );

        Ok(())
    }

    /// Build an HS256-signed DPoP proof with our own header
    /// (jsonwebtoken's `Header` does not expose setting `typ` alongside
    /// the full `jwk` object, so we assemble by hand).
    fn build_dpop_proof(
        secret: &[u8],
        jwk: &Jwk,
        htu: &str,
        htm: &str,
        iat: u64,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let header_json = serde_json::json!({
            "typ": "dpop+jwt",
            "alg": "HS256",
            "jwk": jwk,
        });
        let claims = serde_json::json!({
            "htu": htu,
            "htm": htm,
            "iat": iat,
            "jti": uuid::Uuid::new_v4().to_string(),
        });
        let h_b64 = BASE64_URL.encode(serde_json::to_vec(&header_json)?);
        let p_b64 = BASE64_URL.encode(serde_json::to_vec(&claims)?);
        let signing_input = format!("{h_b64}.{p_b64}");
        let sig = hmac_sha256_b64url(secret, signing_input.as_bytes());
        Ok(format!("{signing_input}.{sig}"))
    }

    /// Minimal HMAC-SHA256, base64url-encoded, no external HMAC crate.
    fn hmac_sha256_b64url(secret: &[u8], msg: &[u8]) -> String {
        const BLOCK: usize = 64;
        let mut key = [0u8; BLOCK];
        if secret.len() > BLOCK {
            let h = Sha256::digest(secret);
            key[..h.len()].copy_from_slice(&h);
        } else {
            key[..secret.len()].copy_from_slice(secret);
        }
        let mut ipad = [0u8; BLOCK];
        let mut opad = [0u8; BLOCK];
        for i in 0..BLOCK {
            ipad[i] = key[i] ^ 0x36;
            opad[i] = key[i] ^ 0x5c;
        }
        let mut inner = Sha256::new();
        inner.update(ipad);
        inner.update(msg);
        let inner_digest = inner.finalize();
        let mut outer = Sha256::new();
        outer.update(opad);
        outer.update(inner_digest);
        BASE64_URL.encode(outer.finalize())
    }

    fn issue_access_token(
        secret: &[u8],
        issuer: &str,
        jkt: &str,
        exp: u64,
        iat: u64,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let claims = SolidOidcClaims {
            iss: issuer.to_string(),
            sub: "https://me.example/profile#me".into(),
            aud: serde_json::json!("solid"),
            exp,
            iat,
            webid: Some("https://me.example/profile#me".into()),
            client_id: Some("client-123".into()),
            cnf: Some(CnfClaim { jkt: jkt.to_string() }),
            scope: Some("openid webid".into()),
        };
        Ok(encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret),
        )?)
    }
}

#[cfg(feature = "oidc")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    run::main()
}

#[cfg(not(feature = "oidc"))]
fn main() {
    eprintln!("oidc_client requires --features oidc (re-run with `--features oidc`)");
}
