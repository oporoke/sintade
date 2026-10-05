use axum::http::HeaderMap;

/// The address rate limits are keyed on.
///
/// The API sits behind a reverse proxy / CDN (docs/design.md §2) that *appends* the address it saw
/// to `X-Forwarded-For`. Everything to the left of that last entry was written by the client and
/// is worth nothing: an attacker who rotates a fake leftmost address would get a fresh rate-limit
/// bucket on every request. So the **rightmost** entry is used. Direct-to-origin traffic without
/// the header shares one `unknown` bucket, the strict way to fail.
pub fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit(',').next())
        .map(|ip| ip.trim())
        .filter(|ip| !ip.is_empty() && ip.len() <= 64)
        .map_or_else(|| "unknown".to_string(), str::to_string)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn with(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_str(value).expect("value"),
        );
        headers
    }

    #[test]
    fn the_proxy_appended_address_is_the_one_used() {
        assert_eq!(client_ip(&with("203.0.113.9")), "203.0.113.9");
        assert_eq!(client_ip(&with("1.2.3.4, 203.0.113.9")), "203.0.113.9");
        assert_eq!(
            client_ip(&with("evil, spoofed ,203.0.113.9 ")),
            "203.0.113.9"
        );
    }

    #[test]
    fn nothing_usable_shares_one_bucket() {
        assert_eq!(client_ip(&HeaderMap::new()), "unknown");
        assert_eq!(client_ip(&with("")), "unknown");
        assert_eq!(client_ip(&with("1.2.3.4,")), "unknown");
        assert_eq!(client_ip(&with(&"9".repeat(65))), "unknown");
    }
}
