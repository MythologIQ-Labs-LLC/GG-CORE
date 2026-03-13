
## 2024-05-24 - [Avoid `format!` in Error Paths]
**Learning:** [Using `format!` inside map_err or loop error handling paths is slow because it eagerly allocates a string, even if the error might not be displayed immediately. Rust `thiserror` allows using Enums and `Box<OriginalError>` to defer string allocation until the error is actually formatted.]
**Action:** [Use structured enum variants (e.g. `BatchItemValidation { index: usize, error: Box<InferenceError> }`) instead of stringified variants when mapping errors over a collection.]
