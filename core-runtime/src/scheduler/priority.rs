//! Request prioritization.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Priority level for inference requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
}

impl From<u8> for Priority {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Low,
            1 => Self::Normal,
            2 => Self::High,
            _ => Self::Critical,
        }
    }
}

/// Item with associated priority for queue ordering.
#[derive(Debug)]
pub struct PrioritizedItem<T> {
    pub priority: Priority,
    pub sequence: u64,
    pub item: T,
}

impl<T> PartialEq for PrioritizedItem<T> {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.sequence == other.sequence
    }
}

impl<T> Eq for PrioritizedItem<T> {}

impl<T> PartialOrd for PrioritizedItem<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for PrioritizedItem<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.priority as u8).cmp(&(other.priority as u8)) {
            Ordering::Equal => other.sequence.cmp(&self.sequence), // Lower sequence = earlier
            ord => ord,
        }
    }
}

/// Priority queue for managing ordered requests.
pub struct PriorityQueue<T> {
    heap: BinaryHeap<PrioritizedItem<T>>,
    next_sequence: u64,
}

impl<T> PriorityQueue<T> {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            next_sequence: 0,
        }
    }

    pub fn push(&mut self, item: T, priority: Priority) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.heap.push(PrioritizedItem {
            priority,
            sequence,
            item,
        });
    }

    pub fn pop(&mut self) -> Option<T> {
        self.heap.pop().map(|p| p.item)
    }

    pub fn peek(&self) -> Option<&T> {
        self.heap.peek().map(|p| &p.item)
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Iterate over items in the queue (not in priority order).
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.heap.iter().map(|p| &p.item)
    }
}

impl<T> Default for PriorityQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}
