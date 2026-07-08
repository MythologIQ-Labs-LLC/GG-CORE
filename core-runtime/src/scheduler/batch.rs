//! Request batching logic.

use super::queue::QueuedRequest;

/// Configuration for batch processing.
#[derive(Debug, Clone)]
pub struct BatchConfig {
    pub max_batch_size: usize,
    pub max_total_tokens: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 8,
            max_total_tokens: 4096,
        }
    }
}

/// A batch of requests to process together.
#[derive(Debug)]
pub struct RequestBatch {
    pub requests: Vec<QueuedRequest>,
    pub total_tokens: usize,
}

impl RequestBatch {
    pub fn new() -> Self {
        Self {
            requests: Vec::new(),
            total_tokens: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.requests.len()
    }

    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }
}

impl Default for RequestBatch {
    fn default() -> Self {
        Self::new()
    }
}

/// Batches requests for efficient processing.
pub struct BatchProcessor {
    config: BatchConfig,
}

impl BatchProcessor {
    pub fn new(config: BatchConfig) -> Self {
        Self { config }
    }

    /// Check if a request can be added to the batch.
    pub fn can_add(&self, batch: &RequestBatch, request: &QueuedRequest) -> bool {
        if batch.len() >= self.config.max_batch_size {
            return false;
        }

        // Estimate token count from prompt bytes (avg ~4 chars per token)
        let estimated_tokens = request.prompt.len().div_ceil(4);
        let new_total = batch.total_tokens + estimated_tokens;
        new_total <= self.config.max_total_tokens
    }

    /// Add a request to the batch.
    pub fn add(&self, batch: &mut RequestBatch, request: QueuedRequest) {
        // Estimate token count from prompt bytes (avg ~4 chars per token)
        let estimated_tokens = request.prompt.len().div_ceil(4);
        batch.total_tokens += estimated_tokens;
        batch.requests.push(request);
    }

    /// Create batches from a list of requests.
    pub fn create_batches(&self, requests: Vec<QueuedRequest>) -> Vec<RequestBatch> {
        let mut batches = Vec::new();
        let mut current_batch = RequestBatch::new();

        for request in requests {
            if !self.can_add(&current_batch, &request) && !current_batch.is_empty() {
                batches.push(current_batch);
                current_batch = RequestBatch::new();
            }
            self.add(&mut current_batch, request);
        }

        if !current_batch.is_empty() {
            batches.push(current_batch);
        }

        batches
    }
}
