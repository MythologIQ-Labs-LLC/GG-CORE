## 2024-03-21 - [Hidden Allocations in to_lowercase()]
**Learning:** In Rust, checking case-insensitive string containment via `name.to_lowercase().contains(&pattern.to_lowercase())` creates hidden `String` allocations on every loop iteration, which can severely degrade performance in tight loops like registry searches.
**Action:** When both strings are ASCII, use `.is_ascii()` as a fast-path and check substrings via `.as_bytes().windows(pattern.len()).any(|w| w.eq_ignore_ascii_case(pattern.as_bytes()))` to eliminate O(n) memory allocation entirely while maintaining correctness.
