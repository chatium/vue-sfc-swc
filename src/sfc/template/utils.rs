//! Port of `compiler-sfc/src/template/templateUtils.ts`.

pub fn is_relative_url(url: &str) -> bool {
    matches!(url.chars().next(), Some('.') | Some('~') | Some('@') | Some('#'))
}

pub fn is_external_url(url: &str) -> bool {
    let u = url.strip_prefix("https:").or_else(|| url.strip_prefix("http:")).unwrap_or(url);
    u.starts_with("//")
}

pub fn is_data_url(url: &str) -> bool {
    url.trim_start().to_ascii_lowercase().starts_with("data:")
}

/// `decodeURIComponent`, falling back to the input on malformed escapes.
pub fn normalize_decoded_import_path(source: &str) -> String {
    match decode_uri_component(source) {
        Some(s) => s,
        None => source.to_string(),
    }
}

fn decode_uri_component(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            let byte = u8::from_str_radix(hex, 16).ok()?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `parseUrl` — returns `(path, hash)` like node's legacy `url.parse`.
pub fn parse_url(url: &str) -> (Option<String>, Option<String>) {
    let mut url = url;
    if url.starts_with('~') {
        url = if url.as_bytes().get(1) == Some(&b'/') {
            &url[2..]
        } else {
            &url[1..]
        };
    }
    match url.find('#') {
        Some(i) => {
            let path = &url[..i];
            let hash = &url[i..];
            (
                if path.is_empty() {
                    None
                } else {
                    Some(path.to_string())
                },
                Some(hash.to_string()),
            )
        }
        None => (
            if url.is_empty() {
                None
            } else {
                Some(url.to_string())
            },
            None,
        ),
    }
}

/// `path.posix.join`
pub fn posix_join(base: &str, rest: &str) -> String {
    let joined = if base.ends_with('/') {
        format!("{base}{rest}")
    } else {
        format!("{base}/{rest}")
    };
    normalize_posix(&joined)
}

fn normalize_posix(p: &str) -> String {
    let is_abs = p.starts_with('/');
    let trailing = p.ends_with('/') && p.len() > 1;
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if let Some(last) = parts.last() {
                    if *last != ".." {
                        parts.pop();
                        continue;
                    }
                }
                if !is_abs {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let mut out = parts.join("/");
    if is_abs {
        out.insert(0, '/');
    }
    if out.is_empty() {
        out = if is_abs { "/".into() } else { ".".into() };
    }
    if trailing && !out.ends_with('/') {
        out.push('/');
    }
    out
}
