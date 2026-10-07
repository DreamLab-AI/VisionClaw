//! Centralized time utilities for VisionClaw
//!
//! This module provides a unified interface for all timestamp operations,
//! replacing scattered Utc::now() calls with consistent, testable functions.

use chrono::{DateTime, Utc};

/// Get current UTC timestamp
/// Wrapper around Utc::now() providing a centralized point for time operations.
/// Use this instead of calling Utc::now() directly.
/// # Examples
/// ```rust,ignore
/// use crate::utils::time;
/// let current_time = time::now();
/// println!("Current time: {}", current_time);
/// ```
#[inline]
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// Get current Unix timestamp in milliseconds
/// Returns the number of milliseconds since Unix epoch (1970-01-01 00:00:00 UTC).
/// Useful for database storage and high-precision timing.
/// # Examples
/// ```rust,ignore
/// use crate::utils::time;
/// let millis = time::timestamp_millis();
/// println!("Milliseconds since epoch: {}", millis);
/// ```
#[inline]
pub fn timestamp_millis() -> i64 {
    Utc::now().timestamp_millis()
}

/// Get current Unix timestamp in seconds
/// Returns the number of seconds since Unix epoch (1970-01-01 00:00:00 UTC).
/// # Examples
/// ```rust,ignore
/// use crate::utils::time;
/// let seconds = time::timestamp_seconds();
/// println!("Seconds since epoch: {}", seconds);
/// ```
#[inline]
pub fn timestamp_seconds() -> i64 {
    Utc::now().timestamp()
}

/// Format DateTime in ISO8601/RFC3339 standard format
/// Produces timestamps like: "2025-11-03T19:45:30.123456789+00:00"
/// This is the standard format for API responses and logging.
/// # Arguments
/// * `dt` - The DateTime to format
/// # Examples
/// ```rust,ignore
/// use crate::utils::time;
/// let timestamp = time::now();
/// let formatted = time::format_iso8601(&timestamp);
/// println!("ISO8601: {}", formatted);
/// ```
pub fn format_iso8601(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

/// Parse ISO8601/RFC3339 timestamp string
/// Parses timestamps in standard ISO8601 format.
/// # Arguments
/// * `s` - The timestamp string to parse
/// # Returns
/// * `Ok(DateTime<Utc>)` - Successfully parsed timestamp
/// * `Err(String)` - Parse error with description
/// # Examples
/// ```rust,ignore
/// use crate::utils::time;
/// match time::parse_iso8601("2025-11-03T19:45:30Z") {
///     Ok(dt) => println!("Parsed: {}", dt),
///     Err(e) => println!("Parse error: {}", e),
/// }
/// ```
pub fn parse_iso8601(s: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| format!("Invalid ISO8601 timestamp: {}", e))
}

/// Calculate milliseconds elapsed since a start time
/// Useful for measuring operation duration and performance tracking.
/// # Arguments
/// * `start` - The starting timestamp
/// # Returns
/// The number of milliseconds between start and now
/// # Examples
/// ```rust,ignore
/// use crate::utils::time;
/// let start = time::now();
/// // ... do some work ...
/// let duration_ms = time::elapsed_ms(&start);
/// println!("Operation took {}ms", duration_ms);
/// ```
pub fn elapsed_ms(start: &DateTime<Utc>) -> i64 {
    let now = Utc::now();
    now.timestamp_millis() - start.timestamp_millis()
}

/// Format timestamp for human-readable logging
/// Produces timestamps like: "2025-11-03 19:45:30.123"
/// # Arguments
/// * `dt` - The DateTime to format
/// # Examples
/// ```rust,ignore
/// use crate::utils::time;
/// let timestamp = time::now();
/// let formatted = time::format_log_time(&timestamp);
/// println!("Log time: {}", formatted);
/// ```
pub fn format_log_time(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration as StdDuration;

    #[test]
    fn test_now() {
        let t1 = now();
        thread::sleep(StdDuration::from_millis(10));
        let t2 = now();
        assert!(t2 > t1, "Time should advance");
    }

    #[test]
    fn test_timestamp_millis() {
        let millis = timestamp_millis();
        assert!(millis > 0, "Timestamp should be positive");

        // Should be roughly current time (after 2020)
        assert!(millis > 1_600_000_000_000, "Timestamp should be recent");
    }

    #[test]
    fn test_timestamp_seconds() {
        let seconds = timestamp_seconds();
        assert!(seconds > 0, "Timestamp should be positive");

        // Should be roughly current time (after 2020)
        assert!(seconds > 1_600_000_000, "Timestamp should be recent");
    }

    #[test]
    fn test_format_iso8601() {
        let dt = now();
        let formatted = format_iso8601(&dt);

        // Should contain date and time components
        assert!(formatted.contains("T"), "Should have date-time separator");
        assert!(formatted.contains(":"), "Should have time separators");

        // Should be parseable back
        assert!(parse_iso8601(&formatted).is_ok(), "Should round-trip");
    }

    #[test]
    fn test_parse_iso8601_valid() {
        let test_cases = vec![
            "2025-11-03T19:45:30Z",
            "2025-11-03T19:45:30.123Z",
            "2025-11-03T19:45:30+00:00",
            "2025-11-03T19:45:30.123456+00:00",
        ];

        for input in test_cases {
            assert!(
                parse_iso8601(input).is_ok(),
                "Should parse valid timestamp: {}",
                input
            );
        }
    }

    #[test]
    fn test_parse_iso8601_invalid() {
        let test_cases = vec!["invalid", "2025-11-03", "19:45:30", "not a timestamp"];

        for input in test_cases {
            assert!(
                parse_iso8601(input).is_err(),
                "Should reject invalid timestamp: {}",
                input
            );
        }
    }

    #[test]
    fn test_elapsed_ms() {
        let start = now();
        thread::sleep(StdDuration::from_millis(50));
        let elapsed = elapsed_ms(&start);

        // Should have elapsed at least 50ms (but allow some tolerance)
        assert!(
            (45..=200).contains(&elapsed),
            "Elapsed time should be reasonable: {}ms",
            elapsed
        );
    }

    #[test]
    fn test_format_log_time() {
        let dt = now();
        let formatted = format_log_time(&dt);

        // Should match expected format
        assert!(formatted.contains(" "), "Should have space separator");
        assert!(formatted.contains(":"), "Should have time separators");
        assert!(formatted.contains("."), "Should have milliseconds");

        // Should be around 23 chars: "2025-11-03 19:45:30.123"
        assert!(
            formatted.len() >= 20 && formatted.len() <= 30,
            "Format length should be reasonable: {}",
            formatted.len()
        );
    }

    #[test]
    fn test_round_trip_formatting() {
        let original = now();
        let formatted = format_iso8601(&original);
        let parsed = parse_iso8601(&formatted).expect("Should parse formatted timestamp");

        // Timestamps should be equal within millisecond precision
        let diff = (original.timestamp_millis() - parsed.timestamp_millis()).abs();
        assert!(diff < 2, "Round-trip should preserve timestamp");
    }
}
