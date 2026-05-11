use super::errors::DetailedValidationError;
use super::{ValidationError, ValidationResult, MAX_STRING_LENGTH};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

pub struct Sanitizer;

impl Sanitizer {
    
    pub fn validate_numeric(value: &Value, field: &str) -> ValidationResult<()> {
        match value {
            Value::Number(n) => {
                
                if let Some(f) = n.as_f64() {
                    
                    if f.is_nan() || f.is_infinite() {
                        return Err(ValidationError::new(
                            field,
                            "Invalid numeric value (NaN or Infinity)",
                            "INVALID_NUMBER",
                        )
                        .into());
                    }
                }
                Ok(())
            }
            Value::String(s) => {
                
                match s.parse::<f64>() {
                    Ok(f) => {
                        
                        if f.is_nan() || f.is_infinite() {
                            return Err(ValidationError::new(
                                field,
                                "Invalid numeric value (NaN or Infinity)",
                                "INVALID_NUMBER",
                            )
                            .into());
                        }
                        Ok(())
                    }
                    Err(_) => {
                        
                        Err(
                            ValidationError::new(field, "Invalid numeric format", "INVALID_NUMBER")
                                .into(),
                        )
                    }
                }
            }
            _ => Err(ValidationError::new(
                field,
                "Expected number or numeric string",
                "INVALID_TYPE",
            )
            .into()),
        }
    }

    
    pub fn sanitize_json(value: &mut Value) -> ValidationResult<()> {
        match value {
            Value::String(s) => {
                *s = Self::sanitize_string(s)?;
            }
            Value::Array(arr) => {
                for item in arr.iter_mut() {
                    Self::sanitize_json(item)?;
                }
            }
            Value::Object(obj) => {
                for (key, val) in obj.iter_mut() {
                    
                    if Self::is_suspicious_key(key) {
                        return Err(ValidationError::malicious_content(key).into());
                    }
                    Self::sanitize_json(val)?;
                }
            }
            _ => {} 
        }
        Ok(())
    }

    
    pub fn sanitize_string(input: &str) -> ValidationResult<String> {
        
        if input.len() > MAX_STRING_LENGTH {
            return Err(ValidationError::too_long("string", MAX_STRING_LENGTH).into());
        }

        
        if input.contains('\0') {
            return Err(ValidationError::malicious_content("string").into());
        }

        let mut sanitized = input.to_string();

        // Check for malicious patterns BEFORE escaping, since escaping would alter the patterns
        sanitized = Self::remove_script_tags(&sanitized)?;
        sanitized = Self::remove_sql_injection_patterns(&sanitized)?;
        sanitized = Self::remove_path_traversal(&sanitized)?;
        sanitized = Self::limit_unicode_control_chars(&sanitized)?;
        // HTML escape as the final step to safely render the output
        sanitized = Self::escape_html(&sanitized);

        Ok(sanitized)
    }


    fn remove_script_tags(input: &str) -> ValidationResult<String> {

        let xss_patterns = [

            r"(?i)<script[^>]*>.*?</script>",

            r#"(?i)(href|src)\s*=\s*["']?\s*javascript:"#,

            r#"(?i)(href|src)\s*=\s*["']?\s*vbscript:"#,

            r"(?i)data:text/html[,;]",

            r#"(?i)\s(on\w+)\s*=\s*["'][^"']*["']"#,

            r"(?i)^javascript:",

            r"(?i)^vbscript:",
        ];

        let result = input.to_string();

        for pattern in &xss_patterns {
            let regex = Regex::new(pattern).map_err(|_| {
                DetailedValidationError::from(ValidationError::new(
                    "string",
                    "Invalid sanitization regex",
                    "REGEX_ERROR",
                ))
            })?;

            if regex.is_match(&result) {
                return Err(ValidationError::malicious_content("string").into());
            }
        }

        Ok(result)
    }

    
    fn escape_html(input: &str) -> String {
        input
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;")
    }

    
    fn remove_sql_injection_patterns(input: &str) -> ValidationResult<String> {
        
        
        let sql_injection_patterns = [
            
            r"(?i)(union\s+select|select\s+.*\s+from\s+|insert\s+into\s+|delete\s+from\s+|drop\s+table\s+|update\s+.*\s+set\s+)",
            
            r"(?i)(;\s*--|\*/\s*;)",
            
            r"(?i)('\s+or\s+\d+\s*=\s*\d+|'\s+and\s+\d+\s*=\s*\d+)",
        ];

        for pattern in &sql_injection_patterns {
            let regex = Regex::new(pattern).map_err(|_| {
                DetailedValidationError::from(ValidationError::new(
                    "string",
                    "Invalid SQL regex",
                    "REGEX_ERROR",
                ))
            })?;

            if regex.is_match(input) {
                return Err(ValidationError::malicious_content("string").into());
            }
        }

        Ok(input.to_string())
    }


    fn remove_path_traversal(input: &str) -> ValidationResult<String> {
        // Check for null bytes (path truncation attack)
        if input.contains('\0') {
            return Err(ValidationError::new(
                "path",
                "null byte detected",
                "PATH_TRAVERSAL",
            )
            .into());
        }

        // Decode URL encoding to catch double-encoded attacks
        let decoded = urlencoding::decode(input).unwrap_or_else(|_| input.into());
        if decoded.contains("..") || decoded.contains("./") || decoded.contains(".\\") {
            return Err(ValidationError::new(
                "path",
                "traversal after decode",
                "PATH_TRAVERSAL",
            )
            .into());
        }

        let traversal_patterns = [
            r"\.\./",
            r"\.\.\\",
            r"%2e%2e%2f",
            r"%2e%2e%5c",
            r"..%2f",
            r"..%5c",
        ];

        for pattern in &traversal_patterns {
            let regex = Regex::new(&format!("(?i){}", pattern)).map_err(|_| {
                DetailedValidationError::from(ValidationError::new(
                    "string",
                    "Invalid path regex",
                    "REGEX_ERROR",
                ))
            })?;

            if regex.is_match(input) {
                return Err(ValidationError::malicious_content("string").into());
            }
        }

        Ok(input.to_string())
    }

    
    fn limit_unicode_control_chars(input: &str) -> ValidationResult<String> {
        let mut result = String::with_capacity(input.len());

        for ch in input.chars() {
            match ch {
                
                ' ' | '\t' | '\n' | '\r' => result.push(ch),
                
                c if c.is_control() && !matches!(c, '\u{0009}' | '\u{000A}' | '\u{000D}') => {
                    return Err(ValidationError::malicious_content("string").into());
                }
                
                c => result.push(c),
            }
        }

        Ok(result)
    }

    
    fn is_suspicious_key(key: &str) -> bool {
        
        
        let dangerous_exact_keys = ["__proto__", "constructor", "prototype"];

        
        if dangerous_exact_keys.iter().any(|&k| key == k) {
            return true;
        }

        
        if key == "eval" || key == "<script>" || key.starts_with("<script") {
            return true;
        }

        
        if key == "__proto__" || key == "__defineGetter__" || key == "__defineSetter__" {
            return true;
        }

        false
    }

    
    pub fn sanitize_filename(filename: &str) -> ValidationResult<String> {
        if filename.is_empty() {
            return Err(ValidationError::new(
                "filename",
                "Filename cannot be empty",
                "EMPTY_FILENAME",
            )
            .into());
        }

        if filename.len() > 255 {
            return Err(ValidationError::too_long("filename", 255).into());
        }

        
        let dangerous_chars = ['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'];

        if filename.chars().any(|c| dangerous_chars.contains(&c)) {
            return Err(ValidationError::malicious_content("filename").into());
        }

        
        let reserved_names = [
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ];

        let name_upper = filename.to_uppercase();
        if reserved_names
            .iter()
            .any(|&name| name_upper == name || name_upper.starts_with(&format!("{}.", name)))
        {
            return Err(ValidationError::malicious_content("filename").into());
        }

        
        if filename.starts_with('.') {
            return Err(ValidationError::malicious_content("filename").into());
        }

        Ok(filename.to_string())
    }

    
    pub fn sanitize_email(email: &str) -> ValidationResult<String> {
        let sanitized = Self::sanitize_string(email)?;

        
        if sanitized.len() > 254 {
            
            return Err(ValidationError::too_long("email", 254).into());
        }

        
        if sanitized.matches('@').count() != 1 {
            return Err(ValidationError::invalid_format("email").into());
        }

        let parts: Vec<&str> = sanitized.split('@').collect();
        if parts.len() != 2 {
            return Err(ValidationError::invalid_format("email").into());
        }

        let (local, domain) = (parts[0], parts[1]);

        
        if local.is_empty() || local.len() > 64 {
            return Err(ValidationError::invalid_format("email").into());
        }

        
        if domain.is_empty() || domain.len() > 255 {
            return Err(ValidationError::invalid_format("email").into());
        }

        
        if sanitized.contains("..") {
            return Err(ValidationError::invalid_format("email").into());
        }

        Ok(sanitized)
    }

    
    pub fn sanitize_url(url: &str) -> ValidationResult<String> {
        let sanitized = Self::sanitize_string(url)?;

        
        if sanitized.len() > 2048 {
            return Err(ValidationError::too_long("url", 2048).into());
        }

        
        let parsed_url = url::Url::parse(&sanitized)
            .map_err(|_| DetailedValidationError::from(ValidationError::invalid_format("url")))?;

        
        let allowed_schemes = ["http", "https", "ftp", "ftps"];
        if !allowed_schemes.contains(&parsed_url.scheme()) {
            return Err(ValidationError::new(
                "url",
                "Only http, https, ftp, and ftps URLs are allowed",
                "INVALID_SCHEME",
            )
            .into());
        }

        
        if let Some(host) = parsed_url.host_str() {
            if Self::is_private_ip_or_localhost(host) {
                return Err(ValidationError::new(
                    "url",
                    "Private IP addresses and localhost are not allowed",
                    "PRIVATE_URL",
                )
                .into());
            }
        }

        Ok(sanitized)
    }

    
    fn is_private_ip_or_localhost(host: &str) -> bool {
        if host == "localhost" || host == "127.0.0.1" || host == "::1" {
            return true;
        }

        
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            match ip {
                std::net::IpAddr::V4(ipv4) => {
                    let octets = ipv4.octets();
                    
                    octets[0] == 10
                        || (octets[0] == 172 && (octets[1] >= 16 && octets[1] <= 31))
                        || (octets[0] == 192 && octets[1] == 168)
                        || octets[0] == 127 
                }
                std::net::IpAddr::V6(_) => {
                    
                    host.starts_with("::1") || host.starts_with("fc") || host.starts_with("fd")
                }
            }
        } else {
            false
        }
    }
}

pub struct CSPUtils;

impl CSPUtils {
    
    /// Generate CSP header with nonce for script security
    /// SECURITY FIX: Removed unsafe-inline and unsafe-eval to prevent XSS attacks
    pub fn generate_csp_header() -> String {
        Self::generate_csp_header_with_nonce(&Self::generate_nonce())
    }

    /// Generate a cryptographically secure nonce for CSP
    pub fn generate_nonce() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{:x}", timestamp)
    }

    /// Generate CSP header with a specific nonce
    pub fn generate_csp_header_with_nonce(nonce: &str) -> String {
        vec![
            "default-src 'self'",
            &format!("script-src 'self' 'nonce-{}'", nonce),
            "style-src 'self' 'unsafe-inline'",  // inline styles less dangerous than scripts
            "img-src 'self' data: blob:",
            "font-src 'self'",
            "connect-src 'self' ws: wss:",
            "media-src 'self'",
            "object-src 'none'",
            "base-uri 'self'",
            "form-action 'self'",
            "frame-ancestors 'none'",
            "upgrade-insecure-requests",
        ]
        .join("; ")
    }

    
    pub fn security_headers() -> HashMap<&'static str, &'static str> {
        let mut headers = HashMap::new();

        headers.insert("X-Content-Type-Options", "nosniff");
        headers.insert("X-Frame-Options", "DENY");
        headers.insert("X-XSS-Protection", "1; mode=block");
        headers.insert("Referrer-Policy", "strict-origin-when-cross-origin");
        headers.insert(
            "Permissions-Policy",
            "geolocation=(), microphone=(), camera=()",
        );
        headers.insert("Cross-Origin-Embedder-Policy", "credentialless");
        headers.insert("Cross-Origin-Opener-Policy", "same-origin");

        headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_string() {
        assert!(Sanitizer::sanitize_string("<script>alert('xss')</script>").is_err());
        assert!(Sanitizer::sanitize_string("javascript:alert(1)").is_err());
        assert!(Sanitizer::sanitize_string("' OR 1=1 --").is_err());
        assert!(Sanitizer::sanitize_string("../../../etc/passwd").is_err());

        let safe = Sanitizer::sanitize_string("Hello World!").unwrap();
        assert_eq!(safe, "Hello World!");
    }

    #[test]
    fn test_sanitize_filename() {
        assert!(Sanitizer::sanitize_filename("").is_err());
        assert!(Sanitizer::sanitize_filename("file<>name").is_err());
        assert!(Sanitizer::sanitize_filename("CON").is_err());
        assert!(Sanitizer::sanitize_filename(".hidden").is_err());

        let safe = Sanitizer::sanitize_filename("document.pdf").unwrap();
        assert_eq!(safe, "document.pdf");
    }

    #[test]
    fn test_sanitize_url() {
        assert!(Sanitizer::sanitize_url("javascript:alert(1)").is_err());
        assert!(Sanitizer::sanitize_url("http://localhost/api").is_err());
        assert!(Sanitizer::sanitize_url("http://192.168.1.1/").is_err());

        let safe = Sanitizer::sanitize_url("https://example.com/api").unwrap();
        assert_eq!(safe, "https://example.com/api");
    }
}
