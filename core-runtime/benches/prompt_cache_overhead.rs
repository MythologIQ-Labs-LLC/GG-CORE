//! PromptCache overhead benchmarks (B-38).
//!
//! Quantifies `PromptCache` per-op cost, especially `find_prefix` across token
//! counts — the datum for the O(N²)→O(N) fix (the old impl re-hashed every prefix;
//! the new one does a single forward SHA256 pass). `find_prefix` is benched on a
//! MISS (no cached prefix) so the full longest-prefix scan is exercised — the
//! worst case where the complexity change is starkest. Model-free / default-feature
//! → CI-safe.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use gg_core::memory::PromptCache;

const SIZES: [usize; 3] = [64, 512, 2048];

fn tokens(n: usize) -> Vec<u32> {
    (0..n as u32).collect()
}

fn bench_hash_tokens(c: &mut Criterion) {
    let mut group = c.benchmark_group("prompt_cache_hash_tokens");
    for n in SIZES {
        let toks = tokens(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("tokens", n), &toks, |b, t| {
            b.iter(|| black_box(PromptCache::hash_tokens(t)))
        });
    }
    group.finish();
}

fn bench_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("prompt_cache_get_hit");
    for n in SIZES {
        let toks = tokens(n);
        let mut cache = PromptCache::new(16);
        cache.insert(&toks, vec![0u8; 64], n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("tokens", n), &toks, |b, t| {
            b.iter(|| black_box(cache.get(t).is_some()))
        });
    }
    group.finish();
}

fn bench_find_prefix_miss(c: &mut Criterion) {
    let mut group = c.benchmark_group("prompt_cache_find_prefix_miss");
    for n in SIZES {
        // Cache holds unrelated short entries; the query's prefixes are absent, so
        // find_prefix scans all N lengths — the O(N²)→O(N) case.
        let mut cache = PromptCache::new(16);
        for k in 0..16u32 {
            cache.insert(&[10_000 + k, 20_000 + k], vec![0u8; 8], 2);
        }
        let toks = tokens(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("tokens", n), &toks, |b, t| {
            b.iter(|| black_box(cache.find_prefix(t).is_some()))
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_hash_tokens,
    bench_get,
    bench_find_prefix_miss
);
criterion_main!(benches);
