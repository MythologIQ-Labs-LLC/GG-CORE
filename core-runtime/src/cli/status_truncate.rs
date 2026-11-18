//! UTF-8-safe bounded display truncation for human-readable CLI status.
//! This is presentation only; it does not rewrite model identifiers on IPC.

/// Truncate with an ASCII ellipsis without slicing through UTF-8 codepoints.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    if max_len <= 3 {
        return ".".repeat(max_len);
    }
    let mut end = max_len - 3;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}
