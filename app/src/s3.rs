use url::Url;

/// Parse an S3-style file URL into `(bucket, key)`.
///
/// Supports path-style and virtual-hosted-style URLs for both custom
/// endpoints and `s3.amazonaws.com`.  Returns `None` if the URL cannot be
/// recognised as S3.
pub fn parse_s3_bucket_key(
    file_url: &str,
    configured_endpoint: Option<&str>,
) -> Option<(String, String)> {
    let parsed = Url::parse(file_url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let path = parsed.path().trim_start_matches('/');

    if let Some(endpoint) = configured_endpoint {
        let endpoint = endpoint.trim().trim_end_matches('/');
        let endpoint_host = Url::parse(endpoint)
            .ok()
            .and_then(|url| url.host_str().map(|v| v.to_ascii_lowercase()))
            .or_else(|| {
                endpoint
                    .strip_prefix("https://")
                    .or_else(|| endpoint.strip_prefix("http://"))
                    .map(|value| value.to_ascii_lowercase())
            })?;

        if host == endpoint_host {
            let mut parts = path.splitn(2, '/');
            let bucket = parts.next()?.trim();
            let key = parts.next()?.trim();
            if bucket.is_empty() || key.is_empty() {
                return None;
            }
            return Some((bucket.to_string(), key.to_string()));
        }

        let suffix = format!(".{endpoint_host}");
        if host.ends_with(&suffix) {
            let bucket = host.trim_end_matches(&suffix).trim();
            if !bucket.is_empty() && !path.is_empty() {
                return Some((bucket.to_string(), path.to_string()));
            }
        }
    }

    if host == "s3.amazonaws.com" {
        let mut parts = path.splitn(2, '/');
        let bucket = parts.next()?.trim();
        let key = parts.next()?.trim();
        if bucket.is_empty() || key.is_empty() {
            return None;
        }
        return Some((bucket.to_string(), key.to_string()));
    }

    if let Some(bucket) = host.strip_suffix(".s3.amazonaws.com") {
        if bucket.is_empty() || path.is_empty() {
            return None;
        }
        return Some((bucket.to_string(), path.to_string()));
    }

    None
}
