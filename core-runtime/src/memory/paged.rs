//! Paged memory allocator for KV-cache storage.
//!
//! Implements vLLM-style paged attention with 16 tokens per page.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Tokens stored per page (vLLM standard).
pub const PAGE_TOKENS: usize = 16;

/// Unique identifier for a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageId(pub usize);

/// Fixed-size page for KV-cache storage.
#[derive(Debug)]
pub struct Page {
    id: PageId,
    keys: Vec<f32>,
    values: Vec<f32>,
    used_slots: usize,
    hidden_dim: usize,
}

impl Page {
    /// Create a new page with given hidden dimension.
    pub fn new(id: PageId, hidden_dim: usize) -> Self {
        let capacity = PAGE_TOKENS * hidden_dim;
        Self {
            id,
            keys: vec![0.0; capacity],
            values: vec![0.0; capacity],
            used_slots: 0,
            hidden_dim,
        }
    }

    /// Write KV pair at the given slot.
    pub fn write(&mut self, slot: usize, keys: &[f32], values: &[f32]) {
        let offset = slot * self.hidden_dim;
        let end = offset + self.hidden_dim;
        self.keys[offset..end].copy_from_slice(keys);
        self.values[offset..end].copy_from_slice(values);
        self.used_slots = self.used_slots.max(slot + 1);
    }

    /// Read keys at the given slot.
    pub fn read_keys(&self, slot: usize) -> &[f32] {
        let offset = slot * self.hidden_dim;
        &self.keys[offset..offset + self.hidden_dim]
    }

    /// Read values at the given slot.
    pub fn read_values(&self, slot: usize) -> &[f32] {
        let offset = slot * self.hidden_dim;
        &self.values[offset..offset + self.hidden_dim]
    }

    pub fn id(&self) -> PageId {
        self.id
    }
    pub fn used_slots(&self) -> usize {
        self.used_slots
    }
    pub fn is_full(&self) -> bool {
        self.used_slots >= PAGE_TOKENS
    }

    /// Reset page for reuse.
    pub fn reset(&mut self) {
        self.used_slots = 0;
    }
}

/// Page table mapping sequence positions to physical pages.
#[derive(Debug)]
pub struct PageTable {
    entries: Vec<Option<PageId>>,
    free_pages: VecDeque<PageId>,
    pages: Vec<Page>,
    next_id: AtomicUsize,
    hidden_dim: usize,
    max_pages: usize,
}

impl PageTable {
    /// Create a new page table.
    pub fn new(hidden_dim: usize, max_pages: usize) -> Self {
        Self {
            entries: Vec::new(),
            free_pages: VecDeque::new(),
            pages: Vec::with_capacity(max_pages),
            next_id: AtomicUsize::new(0),
            hidden_dim,
            max_pages,
        }
    }

    /// Allocate a page for the given sequence position.
    pub fn allocate(&mut self, seq_pos: usize) -> Option<PageId> {
        let page_idx = seq_pos / PAGE_TOKENS;
        self.ensure_entries(page_idx + 1);

        if self.entries[page_idx].is_some() {
            return self.entries[page_idx];
        }

        let page_id = self.get_or_create_page()?;
        self.entries[page_idx] = Some(page_id);
        Some(page_id)
    }

    /// Free pages associated with given IDs.
    pub fn free(&mut self, page_ids: &[PageId]) {
        for &id in page_ids {
            if let Some(page) = self.pages.iter_mut().find(|p| p.id == id) {
                page.reset();
                self.free_pages.push_back(id);
            }
        }
        self.entries.iter_mut().for_each(|e| {
            if let Some(id) = e {
                if page_ids.contains(id) {
                    *e = None;
                }
            }
        });
    }

    /// Get page for reading/writing at position.
    pub fn get(&self, seq_pos: usize) -> Option<&Page> {
        let page_idx = seq_pos / PAGE_TOKENS;
        let page_id = self.entries.get(page_idx)?.as_ref()?;
        self.pages.iter().find(|p| p.id == *page_id)
    }

    /// Get mutable page for writing.
    pub fn get_mut(&mut self, seq_pos: usize) -> Option<&mut Page> {
        let page_idx = seq_pos / PAGE_TOKENS;
        let page_id = self.entries.get(page_idx)?.as_ref()?;
        self.pages.iter_mut().find(|p| p.id == *page_id)
    }

    /// Calculate slot within page for sequence position.
    pub fn slot_in_page(seq_pos: usize) -> usize {
        seq_pos % PAGE_TOKENS
    }

    fn ensure_entries(&mut self, count: usize) {
        while self.entries.len() < count {
            self.entries.push(None);
        }
    }

    fn get_or_create_page(&mut self) -> Option<PageId> {
        if let Some(id) = self.free_pages.pop_front() {
            return Some(id);
        }
        if self.pages.len() >= self.max_pages {
            return None;
        }
        let id = PageId(self.next_id.fetch_add(1, Ordering::SeqCst));
        self.pages.push(Page::new(id, self.hidden_dim));
        Some(id)
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
    pub fn free_count(&self) -> usize {
        self.free_pages.len()
    }
}
