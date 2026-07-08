//! Tier 4 tests: Paged KV-Cache and Continuous Batching.

use gg_core::memory::paged::{Page, PageId, PageTable, PAGE_TOKENS};
use gg_core::scheduler::continuous::{
    BatchSlot, ContinuousBatcher, PendingRequest, RequestId, RequestPhase,
};

// ============================================================================
// Phase 1: Paged KV-Cache Tests
// ============================================================================

#[test]
fn page_table_allocate_returns_unique_ids() {
    let mut table = PageTable::new(64, 10);
    let id1 = table.allocate_page().unwrap();
    let id2 = table.allocate_page().unwrap();
    let id3 = table.allocate_page().unwrap();

    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);
}

#[test]
fn page_table_free_recycles_pages() {
    let mut table = PageTable::new(64, 2);
    let id1 = table.allocate_page().unwrap();
    let _id2 = table.allocate_page().unwrap();

    assert_eq!(table.page_count(), 2);
    assert!(table.allocate_page().is_none());

    table.free(&[id1]);
    assert_eq!(table.free_count(), 1);

    let id3 = table.allocate_page().unwrap();
    assert_eq!(id3, id1);
}

#[test]
fn paged_kv_slot_calculation() {
    assert_eq!(PageTable::slot_in_page(0), 0);
    assert_eq!(PageTable::slot_in_page(15), 15);
    assert_eq!(PageTable::slot_in_page(16), 0);
    assert_eq!(PageTable::slot_in_page(31), 15);
}

#[test]
fn page_write_and_read() {
    let mut page = Page::new(PageId(0), 4);
    let keys = [1.0, 2.0, 3.0, 4.0];
    let values = [5.0, 6.0, 7.0, 8.0];
    page.write(0, &keys, &values);

    assert_eq!(page.read_keys(0), &keys);
    assert_eq!(page.read_values(0), &values);
    assert_eq!(page.used_slots(), 1);
}

#[test]
fn page_is_full_after_16_slots() {
    let mut page = Page::new(PageId(0), 4);
    assert!(!page.is_full());

    for i in 0..PAGE_TOKENS {
        page.write(i, &[0.0; 4], &[0.0; 4]);
    }

    assert!(page.is_full());
}

// ============================================================================
// Phase 2: Continuous Batching Tests
// ============================================================================

#[test]
fn continuous_admits_to_free_slots() {
    let mut batcher = ContinuousBatcher::new(4);

    batcher.enqueue(PendingRequest {
        request_id: RequestId(1),
        prompt_tokens: vec![1, 2, 3],
        max_tokens: 10,
    });
    batcher.enqueue(PendingRequest {
        request_id: RequestId(2),
        prompt_tokens: vec![4, 5],
        max_tokens: 5,
    });

    assert_eq!(batcher.pending_count(), 2);
    assert_eq!(batcher.active_count(), 0);

    let admitted = batcher.admit_pending();
    assert_eq!(admitted.len(), 2);
    assert_eq!(batcher.pending_count(), 0);
    assert_eq!(batcher.active_count(), 2);
}

#[test]
fn continuous_evicts_on_complete() {
    let mut batcher = ContinuousBatcher::new(4);

    batcher.enqueue(PendingRequest {
        request_id: RequestId(1),
        prompt_tokens: vec![1],
        max_tokens: 1,
    });
    batcher.admit_pending();

    if let Some(slot) = batcher.get_slot_mut(0) {
        slot.mark_complete();
    }

    let evicted = batcher.evict_completed();
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0], RequestId(1));
    assert_eq!(batcher.active_count(), 0);
}

#[test]
fn continuous_respects_max_slots() {
    let mut batcher = ContinuousBatcher::new(2);

    for i in 0..5 {
        batcher.enqueue(PendingRequest {
            request_id: RequestId(i),
            prompt_tokens: vec![1],
            max_tokens: 10,
        });
    }

    batcher.admit_pending();
    assert_eq!(batcher.active_count(), 2);
    assert_eq!(batcher.pending_count(), 3);
}

#[test]
fn batch_slot_phase_transitions() {
    let mut slot = BatchSlot::new(RequestId(1), 100, 50);

    assert_eq!(slot.phase, RequestPhase::Prefill);

    slot.finish_prefill();
    assert_eq!(slot.phase, RequestPhase::Decode);

    slot.record_token();
    assert_eq!(slot.tokens_generated, 1);

    slot.mark_complete();
    assert!(slot.is_complete());
}
