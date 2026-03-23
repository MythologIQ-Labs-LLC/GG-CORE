//! KV cache throughput benchmarks.
//!
//! Measures insert/lookup throughput at various cache sizes,
//! eviction overhead under memory pressure, and Q8 encode/decode speed.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use gg_core::memory::kv_cache::{KvCacheConfig, KvCacheManager};
use gg_core::memory::kv_quant::{compute_scale, dequantize, quantize_to, Q8KvStore};

fn make_config(max_pages: usize) -> KvCacheConfig {
    KvCacheConfig {
        hidden_dim: 128,
        max_pages,
        max_seq_len: max_pages * 16,
        num_heads: 8,
        head_dim: 16,
        enable_quantization: true,
        enable_paged: true,
        ..Default::default()
    }
}

fn bench_insert_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("kv_insert_throughput");

    for &pages in &[256, 1024, 4096] {
        group.bench_with_input(
            BenchmarkId::new("pages", pages),
            &pages,
            |b, &p| {
                let config = make_config(p);
                let keys = vec![1.0f32; config.hidden_dim];
                let values = vec![2.0f32; config.hidden_dim];

                b.iter_with_setup(
                    || {
                        let mgr = KvCacheManager::new(config.clone());
                        let seq = mgr.allocate_sequence();
                        (mgr, seq)
                    },
                    |(mgr, seq)| {
                        for _ in 0..64 {
                            mgr.append_kv(seq, &keys, &values).unwrap();
                        }
                    },
                );
            },
        );
    }

    group.finish();
}

fn bench_lookup_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("kv_lookup_throughput");

    for &seq_len in &[64, 256, 1024] {
        group.bench_with_input(
            BenchmarkId::new("seq_len", seq_len),
            &seq_len,
            |b, &n| {
                let config = make_config(4096);
                let mgr = KvCacheManager::new(config.clone());
                let seq = mgr.allocate_sequence();
                let keys = vec![1.0f32; config.hidden_dim];
                let values = vec![2.0f32; config.hidden_dim];
                for _ in 0..n {
                    mgr.append_kv(seq, &keys, &values).unwrap();
                }

                let mut k_out = vec![0.0f32; config.hidden_dim];
                let mut v_out = vec![0.0f32; config.hidden_dim];

                b.iter(|| {
                    let pos = black_box(n / 2);
                    mgr.read_kv(seq, pos, &mut k_out, &mut v_out).unwrap();
                });
            },
        );
    }

    group.finish();
}

fn bench_eviction_pressure(c: &mut Criterion) {
    let mut group = c.benchmark_group("kv_eviction_pressure");
    group.sample_size(30);

    group.bench_function("fill_and_evict", |b| {
        let config = make_config(32);
        let keys = vec![1.0f32; config.hidden_dim];
        let values = vec![2.0f32; config.hidden_dim];

        b.iter_with_setup(
            || KvCacheManager::new(config.clone()),
            |mgr| {
                // Fill cache beyond capacity, triggering eviction
                for i in 0..64_u64 {
                    let seq = mgr.allocate_sequence();
                    for _ in 0..20 {
                        let _ = mgr.append_kv(seq, &keys, &values);
                    }
                    black_box(i);
                }
            },
        );
    });

    group.finish();
}

fn bench_q8_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("q8_encode");

    for &dim in &[128, 512, 4096] {
        let data: Vec<f32> = (0..dim).map(|i| (i as f32) * 0.01).collect();
        let scale = compute_scale(&data);
        let mut out = vec![0u8; dim];

        group.bench_with_input(
            BenchmarkId::new("dim", dim),
            &dim,
            |b, _| b.iter(|| quantize_to(&mut out, black_box(&data), scale)),
        );
    }

    group.finish();
}

fn bench_q8_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("q8_decode");

    for &dim in &[128, 512, 4096] {
        let q_data: Vec<u8> = (0..dim).map(|i| (i % 256) as u8).collect();
        let mut out = vec![0.0f32; dim];

        group.bench_with_input(
            BenchmarkId::new("dim", dim),
            &dim,
            |b, _| b.iter(|| dequantize(black_box(&q_data), &mut out, 0.5)),
        );
    }

    group.finish();
}

fn bench_q8_store_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("q8_store_append");

    for &dim in &[128, 512] {
        let keys: Vec<f32> = (0..dim).map(|i| i as f32 * 0.01).collect();
        let values: Vec<f32> = (0..dim).map(|i| i as f32 * 0.02).collect();

        group.bench_with_input(
            BenchmarkId::new("dim", dim),
            &dim,
            |b, &d| {
                b.iter_with_setup(
                    || Q8KvStore::new(d, 1024),
                    |mut store| {
                        for _ in 0..128 {
                            store.append(&keys, &values);
                        }
                    },
                );
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_insert_throughput,
    bench_lookup_throughput,
    bench_eviction_pressure,
    bench_q8_encode,
    bench_q8_decode,
    bench_q8_store_append,
);
criterion_main!(benches);
