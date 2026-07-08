//! Paged memory allocator for KV-cache storage.
//!
//! Implements vLLM-style paged attention with 16 tokens per page.
//! Pages are owned exclusively by one sequence — the caller is responsible
//! for tracking which PageId belongs to which sequence position.

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

    /// Reset page for reuse, zeroing prior-tenant bytes (remanence hygiene).
    pub fn reset(&mut self) {
        self.used_slots = 0;
        self.keys.iter_mut().for_each(|x| *x = 0.0);
        self.values.iter_mut().for_each(|x| *x = 0.0);
    }
}

/// Pure page pool — callers own exclusive rights to each PageId they allocate.
///
/// No global position→page mapping. The caller tracks which PageId is at which
/// sequence position (via `SequenceEntry.page_ids`). This prevents cross-sequence
/// aliasing where two sequences at the same position would share a page.
#[derive(Debug)]
pub struct PageTable {
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
            free_pages: VecDeque::new(),
            pages: Vec::with_capacity(max_pages),
            next_id: AtomicUsize::new(0),
            hidden_dim,
            max_pages,
        }
    }

    /// Allocate a fresh or recycled page for exclusive caller ownership.
    pub fn allocate_page(&mut self) -> Option<PageId> {
        self.get_or_create_page()
    }

    /// Borrow a page by its id (O(1) — id.0 is the pages-vec index).
    pub fn page(&self, id: PageId) -> Option<&Page> {
        self.pages.get(id.0)
    }

    /// Mutably borrow a page by its id.
    pub fn page_mut(&mut self, id: PageId) -> Option<&mut Page> {
        self.pages.get_mut(id.0)
    }

    /// Free pages by id, resetting each for reuse.
    pub fn free(&mut self, page_ids: &[PageId]) {
        for &id in page_ids {
            if let Some(page) = self.pages.get_mut(id.0) {
                page.reset();
                self.free_pages.push_back(id);
            }
        }
    }

    /// Calculate slot within a page for a sequence position.
    pub fn slot_in_page(seq_pos: usize) -> usize {
        seq_pos % PAGE_TOKENS
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_table_basic_allocation() {
        let mut pt = PageTable::new(128, 10);
        let id = pt.allocate_page();
        assert!(id.is_some());
        assert_eq!(pt.page_count(), 1);
        assert_eq!(pt.free_count(), 0);
    }

    #[test]
    fn test_page_table_free() {
        let mut pt = PageTable::new(128, 10);
        let id = pt.allocate_page().unwrap();

        {
            let page = pt.page_mut(id).unwrap();
            let data = vec![1.0; 128];
            page.write(0, &data, &data);
            assert_eq!(page.used_slots(), 1);
        }

        pt.free(&[id]);

        assert_eq!(pt.free_count(), 1);
        // Page is reset: used_slots and buffers zeroed
        let page = pt.pages.iter().find(|p| p.id == id).unwrap();
        assert_eq!(page.used_slots(), 0);
        assert!(page.read_keys(0).iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_page_table_reuse() {
        let mut pt = PageTable::new(128, 10);
        let id1 = pt.allocate_page().unwrap();
        pt.free(&[id1]);
        assert_eq!(pt.free_count(), 1);

        let id2 = pt.allocate_page().unwrap();
        assert_eq!(id1, id2);
        assert_eq!(pt.free_count(), 0);
        assert_eq!(pt.page_count(), 1);
    }

    #[test]
    fn test_page_table_free_multiple() {
        let mut pt = PageTable::new(128, 10);
        let id1 = pt.allocate_page().unwrap();
        let id2 = pt.allocate_page().unwrap();

        assert_eq!(pt.page_count(), 2);

        pt.free(&[id1, id2]);
        assert_eq!(pt.free_count(), 2);
    }

    #[test]
    fn test_evicted_page_is_zeroed() {
        let mut pt = PageTable::new(4, 10);
        let id = pt.allocate_page().unwrap();
        {
            let page = pt.page_mut(id).unwrap();
            page.write(0, &[9.9; 4], &[8.8; 4]);
        }
        pt.free(&[id]);
        let id2 = pt.allocate_page().unwrap();
        assert_eq!(id, id2, "reused same slot");
        let page = pt.page(id2).unwrap();
        assert!(
            page.read_keys(0).iter().all(|&v| v == 0.0),
            "remanence: keys not zeroed"
        );
        assert!(
            page.read_values(0).iter().all(|&v| v == 0.0),
            "remanence: values not zeroed"
        );
    }
}
