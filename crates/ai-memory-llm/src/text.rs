//! Small text helpers shared by provider implementations.

/// Default per-request embedding input cap (bytes). Matches the historical
/// hard ceiling before `AI_MEMORY_EMBEDDING_MAX_BYTES` was configurable.
pub const DEFAULT_EMBEDDING_MAX_BYTES: usize = 8_000;

/// Truncate to at most `max_bytes` without splitting a UTF-8 codepoint.
pub(crate) fn truncate_with_ellipsis(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }

    let mut end = 0;
    for (idx, ch) in s.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    format!("{}…", &s[..end])
}

/// Return a suffix no longer than `max_bytes`, aligned to a UTF-8 boundary.
pub(crate) fn suffix_within_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let start = s
        .char_indices()
        .map(|(idx, _)| idx)
        .find(|idx| s.len() - idx <= max_bytes)
        .unwrap_or(s.len());
    &s[start..]
}

/// Truncate wiki/query text before sending it to an embedding provider.
///
/// `max_bytes` comes from config (`AI_MEMORY_EMBEDDING_MAX_BYTES`), defaulting
/// to [`DEFAULT_EMBEDDING_MAX_BYTES`].
pub fn truncate_for_embedding(text: &str, max_bytes: usize) -> String {
    const ELLIPSIS_BYTES: usize = "…".len();
    if text.len() <= max_bytes {
        return text.to_string();
    }
    if max_bytes <= ELLIPSIS_BYTES {
        return String::new();
    }
    truncate_with_ellipsis(text, max_bytes - ELLIPSIS_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_ascii_prefix() {
        assert_eq!(truncate_with_ellipsis("abcdef", 3), "abc…");
        assert_eq!(truncate_with_ellipsis("abc", 3), "abc");
    }

    #[test]
    fn truncate_never_splits_utf8() {
        let s = format!("{}é", "x".repeat(1023));
        let truncated = truncate_with_ellipsis(&s, 1024);
        assert!(truncated.ends_with('…'));
        assert_eq!(truncated.chars().last(), Some('…'));
    }

    #[test]
    fn suffix_never_splits_utf8() {
        let s = format!("é{}", "x".repeat(1023));
        assert_eq!(suffix_within_bytes(&s, 1024), "x".repeat(1023));
    }

    #[test]
    fn truncate_for_embedding_caps_long_input() {
        let long = "x".repeat(50_000);
        let out = truncate_for_embedding(&long, DEFAULT_EMBEDDING_MAX_BYTES);
        assert!(out.ends_with('…'));
        assert!(out.len() < long.len());
        assert!(out.len() <= DEFAULT_EMBEDDING_MAX_BYTES);
    }

    #[test]
    fn truncate_for_embedding_respects_configured_cap() {
        let long = "x".repeat(50_000);
        let out = truncate_for_embedding(&long, 32_000);
        assert!(out.ends_with('…'));
        assert!(out.len() <= 32_000);
        assert!(out.len() > DEFAULT_EMBEDDING_MAX_BYTES);
    }
}
