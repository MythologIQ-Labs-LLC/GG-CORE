## 2024-05-24 - Avoid string allocations in string matching
**Learning:** In tight loops, `.to_lowercase().contains(&pattern.to_lowercase())` allocates a lot of memory and hurts performance.
**Action:** Replace allocation-heavy matching with an allocation-free ASCII window search `string.as_bytes().windows(pattern.len()).any(|w| w.eq_ignore_ascii_case(pattern.as_bytes()))`. Always verify `if pattern.is_empty()` first to avoid a runtime panic from `.windows(0)` and make sure `.is_ascii()` is true.
