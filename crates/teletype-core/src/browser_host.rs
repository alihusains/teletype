//! Browser host helpers: turn a URL into a bare host and map known browser
//! hosts onto an app category.
//!
//! Gmail, Slack and Teams in a browser all report the browser's bundle id, so
//! the raw host alone is what disambiguates the webmail/chat surface the user
//! is actually dictating into. The parsing here is pure and table-driven: no
//! OS access, no heuristics beyond the documented host tables.

/// Extract the bare lowercase host from a URL.
///
/// Strips the scheme (which must be `http` or `https`), any userinfo, the port,
/// and the path/query/fragment; a bare host (`mail.google.com/mail/u/0`) is
/// accepted as-is. Returns `None` for empty input, non-http(s) schemes, IP
/// literals and anything that carries no host.
pub fn host_from_url(s: &str) -> Option<String> {
    let input = s.trim();
    if input.is_empty() {
        return None;
    }
    // A scheme is only what precedes "://"; "host:443/path" has no scheme.
    // Any other "<word>:" prefix (mailto:, http: with no slashes) is not a
    // web page host, so it is rejected rather than sliced.
    let rest = if let Some((scheme, after)) = input.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return None;
        }
        after.to_string()
    } else if input
        .split_once(':')
        .is_some_and(|(before, _)| !before.contains('.'))
    {
        return None;
    } else {
        input.to_string()
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.split('@').next_back().unwrap_or(authority);
    let host = host_port
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(host_port)
        .to_ascii_lowercase();
    if host.is_empty() || !host.chars().all(is_host_char) {
        return None;
    }
    if host.starts_with('-') || host.ends_with('-') {
        return None;
    }
    let segments: Vec<&str> = host.split('.').collect();
    if segments.iter().any(|seg| seg.is_empty()) {
        return None;
    }
    if is_ip_literal(&host) {
        return None;
    }
    Some(host)
}

/// Lowercase host character: alphanumerics, `-` and `.` separators only.
fn is_host_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.'
}

/// True for all-numeric dotted hosts (IPv4 literals); IPv6 literals contain
/// characters (brackets, `:`) that the host charset above already rejects.
fn is_ip_literal(host: &str) -> bool {
    let segments: Vec<&str> = host.split('.').collect();
    segments.len() > 1
        && segments.iter().all(|seg| {
            !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()) && seg.parse::<u32>().is_ok()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_scheme_and_bare_host_with_port_do_not_panic() {
        assert_eq!(host_from_url("http:"), None);
        assert_eq!(host_from_url("https:/"), None);
        assert_eq!(host_from_url("mailto:someone@example.com"), None);
        assert_eq!(
            host_from_url("mail.google.com:443/mail"),
            Some("mail.google.com".to_string())
        );
    }

    #[test]
    fn host_from_url_table() {
        let cases: &[(&str, Option<String>)] = &[
            // scheme stripped, bare host + path/query/fragment left to drop
            (
                "https://mail.google.com/mail/u/0",
                Some("mail.google.com".to_string()),
            ),
            ("http://gmail.com", Some("gmail.com".to_string())),
            // port stripped
            (
                "https://mail.google.com:8080/mail/u/0",
                Some("mail.google.com".to_string()),
            ),
            // path, query and fragment stripped
            (
                "https://mail.google.com/mail/u/0?hl=en#top",
                Some("mail.google.com".to_string()),
            ),
            // userinfo stripped
            (
                "https://user:pass@mail.google.com/mail/u/0",
                Some("mail.google.com".to_string()),
            ),
            // uppercase scheme/host normalized to lowercase
            (
                "HTTPS://Mail.Google.COM/Mail/U/0",
                Some("mail.google.com".to_string()),
            ),
            // bare host accepted
            (
                "mail.google.com/mail/u/0",
                Some("mail.google.com".to_string()),
            ),
            // non-http(s) schemes rejected
            ("file:///Users/x", None),
            ("chrome://settings", None),
            ("ftp://mail.google.com", None),
            // IP literals rejected
            ("https://192.168.1.10/", None),
            // empty input rejected
            ("", None),
        ];
        for (input, expected) in cases {
            assert_eq!(host_from_url(input), *expected, "host_from_url({input})");
        }
    }
}
