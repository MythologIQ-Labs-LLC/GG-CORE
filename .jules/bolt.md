## 2025-05-15 - [Pre-allocate String Capacity in Chat Prompt Formatting]
**Learning:** In Rust, repeatedly calling `push_str` on a `String` initialized with `String::new()` can lead to multiple reallocations ($O(N \log N)$ cost). Pre-calculating and allocating the required capacity with `String::with_capacity` reduces this to a single allocation ($O(N)$).
**Action:** Always consider if the total length of a `String` or `Vec` can be reasonably estimated or calculated before entering a loop that populates it.
