//! Log / diagnostic redaction (docs/design-v2.md §15.3, V2-SR-004).
//!
//! Redaction is the **last line of defense**. The supervisor never intentionally
//! prints a secret and then relies on this layer to wipe it (V2-SR-004). The
//! patterns below cover the credential shapes that could leak through child
//! stdout/stderr (Authorization, Bearer, *_API_KEY, PI_HUB_PASSWORD, Telegram
//! bot token, OpenSSH private key block, Cookie).
//!
//! No `regex` dependency: the surface is small and fixed, so targeted scanning
//! keeps the dependency set minimal (AGENTS.md §3).

/// Marker substituted in place of any redacted value.
pub const REDACTED: &str = "[REDACTED]";

/// Redact a single line of captured output. Returns an owned, safe-to-surface
/// string. The function is total — it never panics on malformed input.
pub fn redact_line(input: &str) -> String {
    // First handle full-line structural patterns (header / assignment /
    // private-key block), then sweep the remainder for token-shaped secrets.
    let after_assign = redact_assignments(input);
    let after_header = redact_header_lines(&after_assign);
    let after_key_block = redact_private_key_block(&after_header);
    redact_tokens(&after_key_block)
}

/// Redact `KEY=value` and `export KEY=value` forms for sensitive key names.
fn redact_assignments(input: &str) -> String {
    const SENSITIVE_SUFFIXES: &[&str] = &[
        "_API_KEY",
        "_TOKEN",
        "_SECRET",
        "_PASSWORD",
        "_PASSPHRASE",
        "_PRIVATE_KEY",
    ];
    const SENSITIVE_EXACT: &[&str] = &[
        "PI_HUB_PASSWORD",
        "AUTHORIZATION",
        "COOKIE",
        "API_KEY",
        "TOKEN",
        "SECRET",
        "PASSPHRASE",
    ];

    let line = input.trim_start();
    let is_export = line
        .strip_prefix("export ")
        .map(str::trim_start)
        .unwrap_or(line);
    let bytes = is_export.as_bytes();
    // Read an identifier run ([A-Za-z0-9_]).
    let mut end = 0;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    if end == 0 || end >= is_export.len() || is_export.as_bytes()[end] != b'=' {
        return input.to_string();
    }
    let key = &is_export[..end];
    let key_upper = key.to_ascii_uppercase();
    let sensitive = SENSITIVE_EXACT.iter().any(|e| key_upper == *e)
        || SENSITIVE_SUFFIXES.iter().any(|s| key_upper.ends_with(s));
    if !sensitive {
        return input.to_string();
    }
    // Preserve everything up to and including '='.
    let prefix_end = input.len() - is_export.len() + end + 1;
    let mut out = String::with_capacity(input.len());
    out.push_str(&input[..prefix_end]);
    out.push_str(REDACTED);
    out
}

/// Redact header lines such as `Authorization: ...`, `Cookie: ...`.
fn redact_header_lines(input: &str) -> String {
    const HEADERS: &[&str] = &[
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "x-api-key",
        "x-auth-token",
    ];
    let trimmed = input.trim_start();
    let mut colon = None;
    for (i, b) in trimmed.bytes().enumerate() {
        if b == b':' {
            colon = Some(i);
            break;
        }
        // Header field names are tokens; stop early on whitespace/newline.
        if b == b' ' || b == b'\n' || b == b'\r' || b == b'\t' {
            return input.to_string();
        }
    }
    let Some(colon) = colon else {
        return input.to_string();
    };
    let name = trimmed[..colon].to_ascii_lowercase();
    if !HEADERS.contains(&name.as_str()) {
        return input.to_string();
    }
    let prefix_end = input.len() - trimmed.len() + colon + 1;
    let mut out = String::with_capacity(input.len());
    out.push_str(&input[..prefix_end]);
    out.push(' ');
    out.push_str(REDACTED);
    out
}

/// Replace an OpenSSH-style private key block with a placeholder, in place
/// within the line. A full key is unlikely to fit on one line, but we still
/// neutralize the marker so a block header is never echoed verbatim.
fn redact_private_key_block(input: &str) -> String {
    if input.contains("-----BEGIN")
        && (input.contains("PRIVATE KEY-----") || input.contains("OPENSSH"))
    {
        return format!("{REDACTED} <redacted private key>");
    }
    input.to_string()
}

/// Sweep for token-shaped secrets that may appear inline:
/// - `Bearer <token>`
/// - Telegram bot token: `<digits>:<35+ base64url chars>`
fn redact_tokens(input: &str) -> String {
    let mut out = redact_bearer(input);
    out = redact_telegram_token(&out);
    out
}

/// Replace `Bearer <token>` (token = non-whitespace run) with the marker.
fn redact_bearer(input: &str) -> String {
    const NEEDLE: &str = "Bearer ";
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = find_ignore_ascii_case(rest, NEEDLE) {
        out.push_str(&rest[..idx]);
        out.push_str("Bearer ");
        out.push_str(REDACTED);
        let after = idx + NEEDLE.len();
        // Skip the token (up to the next whitespace or end of line).
        let mut end = after;
        let bytes = rest.as_bytes();
        while end < bytes.len()
            && bytes[end] != b' '
            && bytes[end] != b'\t'
            && bytes[end] != b'\n'
            && bytes[end] != b'\r'
            && bytes[end] != b','
        {
            end += 1;
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Replace a Telegram bot token `<5+ digits>:<30+ base64url-ish chars>` with the
/// marker. Telegram tokens look like `123456789:AA...`.
fn redact_telegram_token(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        let digit_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let digit_run = i - digit_start;
        if digit_run > 0 {
            // Only a `digits:<30+ token chars>` shape counts as a secret.
            let is_token = digit_run >= 5 && i < bytes.len() && bytes[i] == b':' && {
                let mut j = i + 1;
                while j < bytes.len() && is_token_char(bytes[j]) {
                    j += 1;
                }
                j - (i + 1) >= 30
            };
            if is_token {
                let mut j = i + 1;
                while j < bytes.len() && is_token_char(bytes[j]) {
                    j += 1;
                }
                out.push_str(REDACTED);
                i = j;
            } else {
                // Emit the (non-secret) digit run verbatim.
                out.push_str(std::str::from_utf8(&bytes[digit_start..i]).unwrap_or(""));
            }
        } else {
            // Non-digit byte: emit and advance one.
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn is_token_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'='
}

/// Case-insensitive substring search (std lacks `str::find` with a predicate).
fn find_ignore_ascii_case(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    let last = hb.len() - nb.len();
    for start in 0..=last {
        let mut ok = true;
        for (k, &c) in nb.iter().enumerate() {
            if hb[start + k].eq_ignore_ascii_case(&c) {
                continue;
            }
            ok = false;
            break;
        }
        if ok {
            return Some(start);
        }
    }
    None
}

/// Redact every line in a multi-line buffer (used when persisting logs).
pub fn redact_text(input: &str) -> String {
    input
        .split('\n')
        .map(redact_line)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_authorization_header() {
        let out = redact_line("Authorization: Bearer abc.def.ghi");
        assert!(out.starts_with("Authorization:"));
        assert!(out.contains(REDACTED));
        assert!(!out.contains("abc.def.ghi"));
    }

    #[test]
    fn redacts_bearer_inline() {
        let out = redact_line("token=Bearer s3cr3tvaluehere");
        assert!(out.contains(REDACTED));
        assert!(!out.contains("s3cr3tvaluehere"));
    }

    #[test]
    fn redacts_api_key_assignment() {
        let out = redact_line("OPENAI_API_KEY=sk-deadbeef123456");
        assert!(out.ends_with(REDACTED));
        assert!(!out.contains("sk-deadbeef"));
    }

    #[test]
    fn redacts_exported_password() {
        let out = redact_line("export PI_HUB_PASSWORD=hunter2");
        assert!(out.ends_with(REDACTED));
        assert!(!out.contains("hunter2"));
    }

    #[test]
    fn redacts_telegram_token() {
        let out = redact_line("TG_TOKEN=123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef");
        assert!(out.contains(REDACTED));
        assert!(!out.contains("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef"));
    }

    #[test]
    fn redacts_cookie_header() {
        let out = redact_line("Set-Cookie: session=supersecret; HttpOnly");
        assert!(out.contains(REDACTED));
        assert!(!out.contains("supersecret"));
    }

    #[test]
    fn redacts_private_key_marker() {
        let out = redact_line("-----BEGIN OPENSSH PRIVATE KEY-----");
        assert!(out.contains(REDACTED));
        assert!(!out.contains("BEGIN OPENSSH"));
    }

    #[test]
    fn leaves_benign_lines_untouched() {
        let line = "Pi Hub listening on http://127.0.0.1:30142";
        assert_eq!(redact_line(line), line);
    }

    #[test]
    fn leaves_non_secret_assignment_untouched() {
        let line = "PORT=30142";
        assert_eq!(redact_line(line), line);
    }

    #[test]
    fn redact_text_handles_multiline() {
        let input = "line one\nOPENAI_API_KEY=sk-test\nline three";
        let out = redact_text(input);
        assert!(out.contains("line one"));
        assert!(out.contains(REDACTED));
        assert!(!out.contains("sk-test"));
        assert!(out.contains("line three"));
    }

    #[test]
    fn never_panics_on_empty_or_garbled() {
        assert_eq!(redact_line(""), "");
        assert_eq!(redact_line("==="), "===");
        assert_eq!(redact_line("::::"), "::::");
    }

    #[test]
    fn header_detection_requires_colon_prefix() {
        // "Authorization" without a colon should not be mangled.
        let out = redact_line("Authorization granted");
        assert_eq!(out, "Authorization granted");
    }
}
