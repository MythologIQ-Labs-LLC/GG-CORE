# Testing Infrastructure

This directory contains historical and current testing infrastructure for the **GG-CORE standalone inference runtime**. Test maturity depends on the exact source and installed artifact; CoreForge is not a prerequisite.

## Directory Structure

```
testing/
├── configs/           # Test configuration files
│   ├── tier1.json      # Tier 1 baseline config
│   ├── tier2.json      # Tier 2 optimization config
│   ├── tier3.json      # Tier 3 optimization config
│   └── full.json       # Full test suite config
├── scripts/            # Test execution and collection scripts
│   ├── run_benchmarks.sh
│   ├── collect_baseline.sh
│   ├── run_integration.sh
│   └── run_security.sh
└── docs/              # Testing documentation
    └── README.md        # This file
```

## Quick Start

### Run All Tests

```bash
# Run unit and integration tests
cd core-runtime
cargo test

# Run benchmarks (requires models to be staged)
cd core-runtime
cargo bench --features full
```

### Run Specific Test Suite

```bash
# Security tests only
cd core-runtime
cargo test security

# Integration tests only
cd core-runtime
cargo test integration

# Benchmarks only
cd core-runtime
cargo bench --features full
```

## Configuration Files

Each configuration file in `configs/` defines test parameters for different scenarios:

- **tier1.json** - Minimum viable targets (>10 tok/s, <100ms P95)
- **tier2.json** - Competitive targets (>25 tok/s, <20ms P95)
- **tier3.json** - Optimized targets (>50 tok/s, <5ms P95)
- **full.json** - Complete test suite with all features enabled

## Scripts

| Script                | Purpose                                           |
| --------------------- | ------------------------------------------------- |
| `run_benchmarks.sh`   | Execute all benchmark suites                      |
| `collect_baseline.sh` | Collect baseline metrics for regression detection |
| `run_integration.sh`  | Run integration tests with real models            |
| `run_security.sh`     | Run security validation suite                     |

## Model Staging

Real models for testing should be staged in `core-runtime/fixtures/models/`:

```
core-runtime/fixtures/models/
├── onnx/
│   ├── tinybert-classifier.onnx   # ~60MB - Classification tests
│   └── minilm-embedder.onnx      # ~80MB - Embedding tests
└── gguf/
    ├── phi3-mini-q4km.gguf    # ~2.2GB - Generation tests
    └── smollm-360m-q8.gguf     # ~400MB - Fast inference baseline
```

## Performance Baselines

Baseline metrics are stored in `core-runtime/fixtures/baselines/`:

- `baseline_metrics.json` - Current baseline for regression detection
- `tier1_baseline.json` - Tier 1 reference metrics
- `tier2_baseline.json` - Tier 2 reference metrics
- `tier3_baseline.json` - Tier 3 reference metrics

## Test Categories

### Unit Tests

- Type validation
- Input bounds checking
- Error handling
- Memory management

### Integration Tests

- ONNX model loading and inference
- GGUF model loading and generation
- IPC communication
- Scheduler queue management

### Benchmarks

- Inference latency (classification/embedding)
- Generation throughput (tokens/second)
- Memory overhead (RSS ratio)
- Concurrent load handling
- IPC encoding/decoding

### Security Tests

- Input validation (oversized, malformed)
- Path traversal prevention
- Hash verification
- Output filtering
- Sandbox boundary testing

## CI Integration

Testing scripts are designed for GitHub Actions CI:

```yaml
# Example workflow
name: Tests
on: [push, pull_request]
jobs:
  unit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test

  benchmarks:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo bench --features full

  security:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test security
```

## Performance Targets

| Metric             | Tier 1    | Tier 2    | Tier 3    |
| ------------------ | --------- | --------- | --------- |
| Generation         | >10 tok/s | >25 tok/s | >50 tok/s |
| Classification P95 | <100ms    | <20ms     | <5ms      |
| Memory Ratio       | <1.5x     | <1.35x    | <1.25x    |

See [`../docs/CONCEPT.md`](../docs/CONCEPT.md) for detailed performance goals.
