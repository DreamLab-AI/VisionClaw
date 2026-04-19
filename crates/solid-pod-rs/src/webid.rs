//! WebID profile document generation and validation.
//!
//! A WebID profile is an HTML document containing a
//! `<script type="application/ld+json">` block identifying the pod
//! owner as a `foaf:Person` with links to their private/public type
//! indices and a Solid account URL.

/// Render a WebID profile as an HTML document with embedded JSON-LD.
pub fn generate_webid_html(pubkey: &str, name: Option<&str>, pod_base: &str) -> String {
    let display_name = name.unwrap_or("Solid Pod User");
    let pod_url = format!("{pod_base}/pods/{pubkey}/");
    let webid = format!("{pod_base}/pods/{pubkey}/profile/card#me");
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
    "schema:identifier": "did:nostr:{pubkey}"
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

/// Validate that a byte slice is a well-formed WebID profile.
///
/// Checks UTF-8 validity and the presence of a JSON-LD script block
/// that parses as JSON.
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
}
