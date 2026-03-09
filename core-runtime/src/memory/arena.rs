//! Lock-free arena allocator for fast bump allocation.
//!
//! Provides thread-safe memory allocation with O(1) allocation and bulk deallocation.

use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Fixed-size memory arena for fast bump allocation.
/// Thread-safe via atomic bump pointer.
pub struct Arena {
    buffer: Box<[UnsafeCell<u8>]>,
    offset: AtomicUsize,
    capacity: usize,
}

// SAFETY: Arena uses atomic operations for thread-safe allocation.
// The compare_exchange loop ensures unique allocations per thread.
unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

impl Arena {
    /// Create a new arena with given capacity in bytes.
    pub fn new(capacity: usize) -> Self {
        let buffer: Vec<UnsafeCell<u8>> = (0..capacity).map(|_| UnsafeCell::new(0)).collect();
        Self {
            buffer: buffer.into_boxed_slice(),
            offset: AtomicUsize::new(0),
            capacity,
        }
    }

    /// Allocate `size` bytes with given alignment.
    /// Returns None if arena is exhausted.
    pub fn alloc(&self, size: usize, align: usize) -> Option<*mut u8> {
        loop {
            let current = self.offset.load(Ordering::Relaxed);
            let aligned = (current + align - 1) & !(align - 1);
            let new_offset = aligned + size;
            if new_offset > self.capacity {
                return None;
            }
            if self
                .offset
                .compare_exchange_weak(current, new_offset, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return Some(self.buffer[aligned].get());
            }
        }
    }

    /// Reset arena for reuse (bulk deallocation).
    /// SAFETY: Caller must ensure no outstanding references to arena memory.
    pub fn reset(&self) {
        self.offset.store(0, Ordering::Release);
    }

    /// Bytes currently allocated.
    pub fn used(&self) -> usize {
        self.offset.load(Ordering::Relaxed)
    }

    /// Total capacity in bytes.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// A typed slice allocated from an arena.
pub struct ArenaSlice<'a, T> {
    ptr: *mut T,
    len: usize,
    _marker: std::marker::PhantomData<&'a T>,
}

impl<'a, T> ArenaSlice<'a, T> {
    /// Create slice from arena allocation.
    pub fn new(arena: &'a Arena, len: usize) -> Option<Self> {
        let size = len * std::mem::size_of::<T>();
        let align = std::mem::align_of::<T>();
        let ptr = arena.alloc(size, align)? as *mut T;
        Some(Self {
            ptr,
            len,
            _marker: std::marker::PhantomData,
        })
    }

    /// Get immutable slice view.
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: ptr is valid for len elements, lifetime tied to arena
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Get mutable slice view.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: ptr is valid for len elements, we have exclusive access
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// Length of the slice.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if slice is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Pool of arenas for request-scoped allocation.
pub struct ArenaPool {
    arenas: Mutex<VecDeque<Arena>>,
    arena_size: usize,
    max_arenas: usize,
}

impl ArenaPool {
    /// Create a new arena pool.
    pub fn new(arena_size: usize, max_arenas: usize) -> Self {
        Self {
            arenas: Mutex::new(VecDeque::with_capacity(max_arenas)),
            arena_size,
            max_arenas,
        }
    }

    /// Acquire an arena from the pool, or create a new one.
    pub fn acquire(&self) -> Arena {
        let mut guard = self.arenas.lock().unwrap();
        guard
            .pop_front()
            .unwrap_or_else(|| Arena::new(self.arena_size))
    }

    /// Return arena to pool after resetting.
    pub fn release(&self, arena: Arena) {
        arena.reset();
        let mut guard = self.arenas.lock().unwrap();
        if guard.len() < self.max_arenas {
            guard.push_back(arena);
        }
        // Otherwise drop the arena
    }

    /// Number of arenas currently available in pool.
    pub fn available(&self) -> usize {
        self.arenas.lock().unwrap().len()
    }
}
