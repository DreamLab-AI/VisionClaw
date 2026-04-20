//! WebID profile document generation and validation.

/// Render a WebID profile as an HTML document with embedded JSON-LD.
///
/// Omits `solid:oidcIssuer`. Prefer [`generate_webid_html_with_issuer`]
/// for Solid-OIDC flows.
pub fn generate_webid_html(pubkey: &str, name: Option<&str>, pod_base: &str) -> String {
    generate_webid_html_with_issuer(pubkey, name, pod_base, None)
}

/// Render a WebID profile with an optional Solid-OIDC issuer
/// advertised via `solid:oidcIssuer`.
pub fn generate_webid_html_with_issuer(
    pubkey: &str,
    name: Option<&str>,
    pod_base: &str,
    oidc_issuer: Option<&str>,
) -> String {
    let display_name = name.unwrap_or("Solid Pod User");
    let pod_url = format!("{pod_base}/pods/{pubkey}/");
    let webid = format!("{pod_base}/pods/{pubkey}/profile/card#me");
    let issuer_line = match oidc_issuer {
        Some(iss) => format!(r#","solid:oidcIssuer":{{"@id":"{iss}"}}"#),
        None => String::new(),
    };
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>{display_name}</title>
  <script type="application/ld+json">
  {{
    "@context": {{
      "foaf": "http://xmlns.com/foaf/0.1/",
      "solid": "http://www.w3.org/ns/solid/terms#",
      "schema": "http://schema.org/"
    }},
    "@id": "{webid}",
    "@type": "foaf:Person",
    "foaf:name": "{display_name}",
    "solid:account": "{pod_url}",
    "solid:privateTypeIndex": "{pod_url}settings/privateTypeIndex",
    "solid:publicTypeIndex": "{pod_url}settings/publicTypeIndex",
    "schema:identifier": "did:nostr:{pubkey}"{issuer_line}
  }}
  </script>
</head>
<body>
  <h1>{display_name}</h1>
  <p>WebID: <a href="{webid}">{webid}</a></p>
  <p>Pod: <a href="{pod_url}">{pod_url}</a></p>
</body>
</html>"#
    )
}

/// Follow-your-nose discovery — extract `solid:oidcIssuer` from a
/// WebID HTML document. Returns `Ok(None)` when the profile does not
/// advertise an issuer.
pub fn extract_oidc_issuer(data: &[u8]) -> Result<Option<String>, String> {
    let text = std::str::from_utf8(data)
        .map_err(|_| "WebID profile must be valid UTF-8".to_string())?;
    // Locate the JSON-LD block.
    let start = match text.find("application/ld+json") {
        Some(s) => s,
        None => return Ok(None),
    };
    let tag_end = match text[start..].find('>') {
        Some(e) => e,
        None => return Ok(None),
    };
    let json_start = start + tag_end + 1;
    let script_end = match text[json_start..].find("</script>") {
        Some(e) => e,
        None => return Ok(None),
    };
    let json_str = text[json_start..json_start + script_end].trim();
    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("WebID JSON-LD parse error: {e}"))?;
    let issuer = value.get("solid:oidcIssuer").or_else(|| {
        value
            .get("http://www.w3.org/ns/solid/terms#oidcIssuer")
    });
    match issuer {
        Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
        Some(serde_json::Value::Object(m)) => {
            if let Some(serde_json::Value::String(s)) = m.get("@id") {
                Ok(Some(s.clone()))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

/// Validate that a byte slice is a well-formed WebID profile.
pub fn validate_webid_html(data: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(data)
        .map_err(|_| "WebID profile must be valid UTF-8".to_string())?;
    if !text.contains("application/ld+json") {
        return Err(
            "WebID profile must contain a <script type=\"application/ld+json\"> block".to_string(),
        );
    }
    if let Some(start) = text.find("application/ld+json") {
        if let Some(tag_end) = text[start..].find('>') {
            let json_start = start + tag_end + 1;
            if let Some(script_end) = text[json_start..].find("</script>") {
                let json_str = text[json_start..json_start + script_end].trim();
                serde_json::from_str::<serde_json::Value>(json_str)
                    .map_err(|e| format!("Invalid JSON-LD in WebID profile: {e}"))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_pubkey() {
        let html = generate_webid_html("abc123", None, "https://pods.example.com");
        assert!(html.contains("abc123"));
        assert!(html.contains("did:nostr:abc123"));
    }

    #[test]
    fn validate_accepts_valid() {
        let html = generate_webid_html("abc", Some("Alice"), "https://pods.example.com");
        assert!(validate_webid_html(html.as_bytes()).is_ok());
    }

    #[test]
    fn validate_rejects_missing_jsonld() {
        let html = "<!DOCTYPE html><html><body>no ld+json</body></html>";
        assert!(validate_webid_html(html.as_bytes()).is_err());
    }

    #[test]
    fn generate_with_issuer_embeds_oidc_triple() {
        let html = generate_webid_html_with_issuer(
            "abc",
            Some("Alice"),
            "https://pods.example.com",
            Some("https://op.example"),
        );
        assert!(html.contains("solid:oidcIssuer"));
        assert!(html.contains("https://op.example"));
    }

    #[test]
    fn extract_oidc_issuer_returns_issuer_id() {
        let html = generate_webid_html_with_issuer(
            "abc",
            Some("Alice"),
            "https://pods.example.com",
            Some("https://op.example"),
        );
        let iss = extract_oidc_issuer(html.as_bytes()).unwrap();
        assert_eq!(iss.as_deref(), Some("https://op.example"));
    }

    #[test]
    fn extract_oidc_issuer_absent_returns_none() {
        let html =
            generate_webid_html_with_issuer("abc", Some("Alice"), "https://p", None);
        let iss = extract_oidc_issuer(html.as_bytes()).unwrap();
        assert!(iss.is_none());
    }
}
