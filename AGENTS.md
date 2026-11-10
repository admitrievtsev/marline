# AGENTS.md

Marline — a Rust library implementing Similarity-Based Chunking (SBC) for data deduplication, built on top of
ChunkFS (public GitHub dependency). Build and run requirements: see README.md.

## 1. Repository layout and crate map

- `marline` — example application and benchmark driver (`src/main.rs`, run with `cargo run --bin marline`); depends
  on `marline_scrub`, `marline_sketcher` and `chunkfs`
- `marline_scrub` — core SBC pipeline: `SBCScrubber`, `SBCMap`, encoders (`GdeltaEncoder`, `XdeltaEncoder`,
  `ZdeltaEncoder`, `DdeltaEncoder`, `LevenshteinEncoder`), decoders, clusterers (`GraphClusterer`, `EqClusterer`)
- `marline_sketcher` — similarity hashing: `SBCHash`/`SBCHasher` traits, `AronovichHasher`, `OdessHasher`,
  `SpeedSketchHasher`, Broder's method
- `marline_delta` — delta-encoding utilities shared by the other crates
- `marline_palantir` — Palantir pipeline: lifecycle tiers, super-feature generator, FP/SF tables, storage backends
  (mock RocksDB), metadata manager; binaries `palantir_backup_bench`, `palantir_metrics`
- `marline_index` — sketch-based similarity search index: `SketchIndexApi` trait, `InvertedSketchIndex`,
  `HeuristicInvertedSketchIndex`, `IndexStorage` storage trait; criterion bench `benches/throughput.rs` (harness=false)

Notes:

- The `chunkfs` dependency is fetched from public GitHub (`https://github.com/Piletskii-Oleg/chunkfs`) — it is not
  part of this workspace; do not vendor it.
- All other dependencies are declared once in `[workspace.dependencies]` of the root `Cargo.toml` (`rand`, `rayon`,
  `zstd`, `thiserror`, `huffman-compress`, `bit-vec`, `log`, `fasthash`); crates reference them with
  `x.workspace = true`. Add new dependencies there, not to individual manifests.
- `Cargo.lock` is not committed (library workspace, see `.gitignore`); do not add it back.
- `marline_sketcher` uses `edition = "2024"` (needs Rust >= 1.85) while the rest of the workspace is edition 2021 —
  keep new crates on `edition.workspace = true` unless there is a reason not to.
- Most crates set `[profile.release] debug = true` — keep it.

## 2. Build

CI runs the latest stable Rust on `ubuntu-latest` (README badge says 1.70+, but the edition-2024 crate effectively
requires 1.85+). No system packages are needed — all dependencies come from crates.io and public git.

```sh
cargo build --all-features
```

- Warnings are treated as errors: `.cargo/config.toml` sets `[build] rustflags = ["-D", "warnings"]`; CI runs
  `cargo clippy --all-targets --tests -- -D warnings`.
- Formatting: `rustfmt` with `use_small_heuristics = "Max"` (`.rustfmt.toml`); CI runs `cargo fmt -- --check`.

## 3. Common commands

Full workspace builds are fast (small crates), but iterate on a single crate for speed:

```sh
cargo build -p marline_scrub
cargo test -p marline_scrub            # unit + integration tests
cargo clippy -p marline_scrub -- -D warnings
cargo fmt --check
```

What CI runs on every push (`.github/workflows/rust.yml`):

```sh
cargo fmt -- --check
cargo clippy --all-targets --tests -- -D warnings
cargo build --all-features --verbose
cargo test --verbose
cargo run --bin marline --verbose --release
```

## 4. Code conventions

### Style

- Crate-level `//!` doc comments (see `marline_index/src/lib.rs`); public items get `///` docs; trait methods are
  documented with a description of the contract (`SBCHash::next_hash`).
- No license headers in `.rs` files — the Apache 2.0 `LICENSE` lives at the repo root; do not add per-file headers.
- Explicit imports (`use x::y`); avoid glob imports except in tests.  
- `#[derive(Debug)]` on plain data structs; `thiserror::Error` for error enums.
- `unsafe` is rare and must always carry a `// SAFETY:` comment explaining the invariant.

### Error handling

- Library crates: `thiserror` enums — `#[derive(Error, Debug)]`, doc-commented variants, `#[error("...")]` with
  context. Examples: `IndexError` in `marline_index/src/index/error.rs`, `PalantirError` in
  `marline_palantir/src/error.rs`.
- Binaries/examples: `std::io::Result<()>`-style returns from `main()`.
- Functionality that can fail per-call should return `Result`, not panic — validate inputs before destructive
  operations.

### Logging

- `log` crate macros (`debug!`, `info!`, ...) — the only logging facade in the workspace; do not introduce another.

## 5. Testing conventions

- **Unit tests**: inline `#[cfg(test)] mod tests` in the same file with `fn test_...` names — new logic must come
  with tests in this style.
- **Integration tests** live in `tests/` at the crate root and exercise the public API:
  `marline_scrub/tests/{gdelta_tests,sbc_tests}.rs`, `marline_palantir/tests/palantir_pipeline_tests.rs`.
- **Benchmarks**: criterion in `benches/` (`marline_index/benches/throughput.rs` with `harness = false`); data
  format is part of the benchmark, so keep them runnable with `cargo bench -p marline_index`.
- CI runs `cargo test --verbose` on every push.

## 6. Git workflow and commits

- Commit messages follow **conventional commits with a scope** (recent history):

  ```
  feat(scrub): add SpeedSketch-based delta encoding
  refactor(sketcher): hide the GEAR table behind a gear() accessor
  test(scrub): cover gdelta round-trip, sketch skip and zero-match clustering
  ci: update run job due to new project structure
  ```

  The early history does not follow this style — do not copy it; new commits should.

- PRs are merged via merge commits (`Merge pull request #N from ...`); keep branches focused on a single change.

## 7. Code review

Reviewers check, in priority order: correctness and invariants (error handling, no data loss in encode/decode
paths), API design (minimal, documented public surface), and performance (this is a performance-focused library —
no needless allocations or repeated hashing in hot paths such as encoder/clusterer loops).

## 8. Documentation

- `README.md` — primary user documentation (features, API overview, quick start, config tables). Update it for
  user-visible changes.
- `marline_index/README.md` — crate-level README; give non-trivial crates one.
- No CHANGELOG exists yet; do not create one unless asked.

## 9. Quick checklist before finishing a change

1. `cargo fmt --check` clean.
2. `cargo clippy --all-targets --tests -- -D warnings` clean (workspace-wide, as CI does).
3. New logic has unit tests (`#[cfg(test)] mod tests`); larger features get an integration test in `tests/`.
4. Public items documented (`///`, `//!` for the crate); no license header needed.
5. Commit message conventional with scope; only task-related files staged.
