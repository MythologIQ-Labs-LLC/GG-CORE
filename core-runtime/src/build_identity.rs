//! Producer-owned compile-time identity for exact Git/Cargo consumers (#113).
//!
//! Pre-v1 additive API. A Git dependency reports its own source revision,
//! never the embedding application's GITHUB_SHA. A source archive without
//! Git metadata must supply GG_CORE_BUILD_COMMIT or report "unknown".
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMMIT: &str = env!("VERGEN_GIT_SHA");
pub const BUILD_DATE: &str = env!("VERGEN_BUILD_DATE");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_identity_or_explicit_unknown() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
        assert_eq!(BUILD_DATE.len(), 10);
        assert!(COMMIT == "unknown" || COMMIT.len() == 40);
        if COMMIT != "unknown" {
            assert!(COMMIT.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }
}
