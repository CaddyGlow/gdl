//! Pure functions for HTTP header parsing and cache control.
//!
//! This module contains testable logic for extracting and validating HTTP headers
//! related to caching and conditional requests.

use reqwest::header::{CACHE_CONTROL, ETAG, HeaderMap, HeaderValue, LAST_MODIFIED};

/// Extracts the ETag header value from response headers.
///
/// Returns None if the header is missing or cannot be parsed as a string.
pub fn extract_etag(headers: &HeaderMap) -> Option<String> {
    headers
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

/// Extracts the Last-Modified header value from response headers.
///
/// Returns None if the header is missing or cannot be parsed as a string.
pub fn extract_last_modified(headers: &HeaderMap) -> Option<String> {
    headers
        .get(LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

/// Extracts the Cache-Control header value from response headers.
///
/// Returns None if the header is missing or cannot be parsed as a string.
pub fn extract_cache_control(headers: &HeaderMap) -> Option<String> {
    headers
        .get(CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

/// Container for cache-related headers extracted from an HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheHeaders {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub cache_control: Option<String>,
}

/// Extracts all cache-related headers from an HTTP response.
pub fn extract_cache_headers(headers: &HeaderMap) -> CacheHeaders {
    CacheHeaders {
        etag: extract_etag(headers),
        last_modified: extract_last_modified(headers),
        cache_control: extract_cache_control(headers),
    }
}

/// Determines if a response is cacheable based on status code and cache-control directives.
///
/// A response is cacheable if:
/// - Status code is 200 (OK) or 304 (Not Modified)
/// - Cache-Control does not contain "no-cache" or "no-store"
///
/// # Examples
///
/// ```
/// use gdl::http::is_cacheable;
///
/// assert!(is_cacheable(200, None));
/// assert!(is_cacheable(304, None));
/// assert!(!is_cacheable(404, None));
/// assert!(!is_cacheable(200, Some("no-cache")));
/// assert!(!is_cacheable(200, Some("no-store")));
/// ```
pub fn is_cacheable(status_code: u16, cache_control: Option<&str>) -> bool {
    if status_code != 200 && status_code != 304 {
        return false;
    }

    if let Some(cc) = cache_control {
        if cc.contains("no-cache") || cc.contains("no-store") {
            return false;
        }
    }

    true
}

/// Determines if a response has cache validation headers (ETag or Last-Modified).
///
/// These headers allow for conditional requests to revalidate stale cache entries.
pub fn has_cache_validation_headers(etag: Option<&str>, last_modified: Option<&str>) -> bool {
    etag.is_some() || last_modified.is_some()
}

/// Calculates the cache Time-To-Live (TTL) in seconds from Cache-Control max-age.
///
/// Returns the default TTL if max-age is not found or cannot be parsed.
///
/// # Arguments
///
/// * `cache_control` - The Cache-Control header value
/// * `default_ttl` - Default TTL to use if max-age is not specified
///
/// # Examples
///
/// ```
/// use gdl::http::calculate_cache_ttl;
///
/// assert_eq!(calculate_cache_ttl(Some("max-age=3600"), 60), 3600);
/// assert_eq!(calculate_cache_ttl(Some("max-age=3600, public"), 60), 3600);
/// assert_eq!(calculate_cache_ttl(Some("public"), 60), 60);
/// assert_eq!(calculate_cache_ttl(None, 60), 60);
/// ```
pub fn calculate_cache_ttl(cache_control: Option<&str>, default_ttl: u64) -> u64 {
    cache_control
        .and_then(|cc| {
            cc.split(',')
                .find(|s| s.trim().starts_with("max-age="))
                .and_then(|s| s.trim().strip_prefix("max-age="))
                .and_then(|s| s.trim().parse::<u64>().ok())
        })
        .unwrap_or(default_ttl)
}

/// Determines if a cached response is still fresh based on age and TTL.
///
/// # Arguments
///
/// * `cached_at` - Timestamp when the response was cached (seconds since epoch)
/// * `current_time` - Current timestamp (seconds since epoch)
/// * `ttl` - Time-To-Live in seconds
///
/// # Examples
///
/// ```
/// use gdl::http::is_cache_fresh;
///
/// assert!(is_cache_fresh(1000, 1030, 60));  // 30s old, TTL 60s
/// assert!(!is_cache_fresh(1000, 1100, 60)); // 100s old, TTL 60s
/// assert!(is_cache_fresh(1000, 1000, 60));  // Just cached
/// ```
pub fn is_cache_fresh(cached_at: u64, current_time: u64, ttl: u64) -> bool {
    let age = current_time.saturating_sub(cached_at);
    age <= ttl
}

/// Parses a header value to a u64 integer.
///
/// Used for parsing numeric header values like Content-Length.
pub fn parse_header_u64(value: Option<&HeaderValue>) -> Option<u64> {
    value
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderMap;

    fn create_headers_with(tuples: Vec<(&str, &str)>) -> HeaderMap {
        use reqwest::header::HeaderName;
        let mut headers = HeaderMap::new();
        for (key, value) in tuples {
            let header_name: HeaderName = key.parse().unwrap();
            headers.insert(header_name, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn test_extract_cache_headers() {
        // All headers present
        let headers = create_headers_with(vec![
            ("etag", "\"abc123\""),
            ("last-modified", "Mon, 01 Jan 2024 00:00:00 GMT"),
            ("cache-control", "max-age=3600"),
        ]);
        let cache_headers = extract_cache_headers(&headers);
        assert_eq!(cache_headers.etag, Some("\"abc123\"".to_string()));
        assert_eq!(
            cache_headers.last_modified,
            Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string())
        );
        assert_eq!(
            cache_headers.cache_control,
            Some("max-age=3600".to_string())
        );

        // All headers missing
        let empty = HeaderMap::new();
        let cache_headers = extract_cache_headers(&empty);
        assert_eq!(cache_headers.etag, None);
        assert_eq!(cache_headers.last_modified, None);
        assert_eq!(cache_headers.cache_control, None);
    }

    #[test]
    fn test_is_cacheable() {
        // Cacheable responses
        assert!(is_cacheable(200, None));
        assert!(is_cacheable(304, None));
        assert!(is_cacheable(200, Some("max-age=3600, public")));

        // Not cacheable
        assert!(!is_cacheable(404, None)); // Wrong status
        assert!(!is_cacheable(200, Some("no-store")));
        assert!(!is_cacheable(200, Some("no-cache")));
    }

    #[test]
    fn test_has_cache_validation_headers() {
        assert!(has_cache_validation_headers(Some("\"abc123\""), None));
        assert!(has_cache_validation_headers(
            None,
            Some("Mon, 01 Jan 2024 00:00:00 GMT")
        ));
        assert!(has_cache_validation_headers(
            Some("\"abc123\""),
            Some("Mon, 01 Jan 2024 00:00:00 GMT")
        ));
        assert!(!has_cache_validation_headers(None, None));
    }

    #[test]
    fn test_calculate_cache_ttl() {
        // Parse from header
        assert_eq!(calculate_cache_ttl(Some("max-age=3600"), 60), 3600);
        assert_eq!(calculate_cache_ttl(Some("max-age=3600, public"), 60), 3600);

        // Use default
        assert_eq!(calculate_cache_ttl(None, 60), 60);
        assert_eq!(calculate_cache_ttl(Some("public"), 120), 120);
    }

    #[test]
    fn test_is_cache_fresh() {
        assert!(is_cache_fresh(1000, 1030, 60)); // Within TTL
        assert!(is_cache_fresh(1000, 1060, 60)); // At boundary
        assert!(!is_cache_fresh(1000, 1061, 60)); // Expired
    }

    #[test]
    fn test_parse_header_u64() {
        let value = HeaderValue::from_str("12345").unwrap();
        assert_eq!(parse_header_u64(Some(&value)), Some(12345));
        assert_eq!(parse_header_u64(None), None);
    }
}
