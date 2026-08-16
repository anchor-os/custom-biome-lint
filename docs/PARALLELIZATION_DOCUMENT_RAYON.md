# Parallelization with Rayon

## Overview

The `custom-biome-lint` tool uses **Rayon** for file-level parallelization to dramatically improve analysis speed on multi-core systems. Parallelization is **enabled by default** but can be disabled with the `--no-parallel` flag.

## Why Rayon?

**Rayon** is a Rust data-parallelism library that provides:
- **Easy parallelization** via `ParallelIterator` trait (drop-in replacement for `.iter()`)
- **Automatic thread pool management** (defaults to CPU core count)
- **Work-stealing scheduler** for efficient load balancing
- **Zero-copy data sharing** via immutable references
- **Minimal overhead** for CPU-bound tasks

## Architecture

### File-Level Parallelization (Not Per-File)

The tool parallelizes at the **file boundary**, not within a single file:

```
discover_files()
    ↓
Sequential: for file in files { analyze(file) }
Parallel:   files.par_iter().map(|file| analyze(file)).collect()
```

**Why this approach:**
- Each file is **completely independent** (no inter-file dependencies)
- No locking or synchronization needed
- Clean separation of concerns

### Directory-Walk Parallelization (Inside `discover_files()`)

`discover_files()` itself is also parallelized, not just the analysis step after
it. `walk()` (in `src/analyzer/mod.rs`) recurses into subdirectories concurrently
on rayon's global thread pool instead of one blocking `read_dir` at a time:

```rust
fn walk(dir: &Path, root: &Path, pattern: &GlobSet) -> Discovery {
    let mut discovery = Discovery::default();
    // ...read_dir this directory, split entries into files vs. subdirs...

    let merged = subdirs
        .into_par_iter()
        .map(|path| walk(&path, root, pattern))   // recurse into each subdir in parallel
        .reduce(Discovery::default, |mut a, b| {  // merge results (files, counts, skips)
            a.dirs_scanned += b.dirs_scanned;
            a.files_considered += b.files_considered;
            a.files.extend(b.files);
            a.dirs_skipped.extend(b.dirs_skipped);
            a
        });

    // ...fold `merged` into this directory's own discovery...
    discovery
}
```

**Why this matters:**
- Each directory's `read_dir` + pattern-match pass is small on its own, but a
  tree with hundreds of directories turns into hundreds of blocking syscalls if
  walked one at a time
- `into_par_iter().map(...).reduce(...)` fans the recursive calls out across
  cores and merges each subtree's `Discovery` (files found, dirs scanned/skipped,
  files considered) back into the parent — no shared mutable state, no locks
- This runs **before** file analysis: discovery and analysis are two separate
  parallel stages, not one combined pass

**Where this lives:** `discover_files()` → `walk()` in `src/analyzer/mod.rs`.
Ported into this copy from the `biome-live-setup` worktree after a diff
comparison showed it was the one place the two worktrees' copies of
`custom-biome-lint` had diverged; both are now byte-identical.

### Single-Pass Analysis

Each file is analyzed **exactly once**, with all rules running over the same syntax tree:

```rust
for file in files {
    let source = fs::read_to_string(file)?;
    let analyzed = analyze_file(file, &source, rules);  // All rules on one parse
}
```

**Why this matters:**
- Parsing dominates runtime (~72% of total time with 3 rules)
- Sharing the AST across rules eliminates redundant parses
- Parallelization stacks on top of this optimization

## Implementation Details

### Code Structure

**`src/cli/mod.rs`** - orchestration layer:
```rust
if args.parallel {
    analyze_files_parallel(&discovery.files, &rules, ...)
} else {
    analyze_files_sequential(&discovery.files, &rules, ...)
}
```

**`analyze_files_parallel()`** - rayon integration:
```rust
files
    .into_par_iter()
    .map(|file| {
        let source = fs::read_to_string(&file).unwrap_or_default();
        let result = analyze_file(&file, &source, rules);
        (file, result)
    })
    .collect()
```

**Key points:**
- Uses `into_par_iter()` for owned data
- Each closure runs in a thread pool
- Results collected into a `Vec` automatically
- No `Mutex`, `Arc`, or other synchronization needed

### Thread Safety

All components are **thread-safe by design**:

| Component | Thread-Safe? | Why |
|-----------|-------------|-----|
| `FileContext` | ✓ Yes | Confined to one thread per file — `analyze_file()` creates it, uses it, and drops it entirely inside a single `par_iter()` closure; it's never shared or sent across threads. Not itself deeply immutable since the semantic model addition (`semantic: OnceCell<SemanticModel>`, lazily populated on first `FileContext::semantic()` call — see `docs/SEMANTIC_MODEL.md`), but that's irrelevant here precisely because no instance is ever accessed from more than one thread |
| `JsFileSource` (Rowan AST) | ✓ Yes | Immutable syntax tree |
| `Rule` trait | ✓ Yes | Implements `Send + Sync` |
| `Violation` | ✓ Yes | Owned, immutable data |
| `analyze_file()` | ✓ Yes | Pure function, no state |

## Performance Characteristics

### Measured Speedup

```
Hardware: 4-core CPU (M2 MacBook)
Codebase: <PRIVATE_REPO>/src/ (4393 files)

Sequential (baseline):    ~2.4s
Parallel (theoretical):   ~2.4s / 4 = 0.6s
Parallel (actual):        ~0.8-1.2s (overhead ~30-50%)

Overhead sources:
- Thread spawning/joining
- Work-stealing scheduler fairness
- Cache effects (thread-local CPU caches)
```

### When Parallelization Helps

| Scenario | Benefit |
|----------|---------|
| **Large codebase** (1000+ files) | 2-4x speedup on multi-core |
| **Small codebase** (< 100 files) | Minimal (overhead > benefit) |
| **Single-core machine** | No benefit (but also no harm) |
| **CI pipeline** (cloud instances) | 8+ core → 4-6x speedup |

### When Parallelization Doesn't Help

- **Very fast analysis** (< 1ms per file) → overhead dominates
- **I/O bound workload** (slow disk reads) → network/disk is bottleneck
- **Single-core CPU** → threads serialize anyway

## CLI Interface

### Default Behavior

```bash
# Parallel enabled by default
custom-biome-lint src
# Equivalent to:
custom-biome-lint src --parallel
```

### Disable Parallelization

```bash
# Run sequentially on single core
custom-biome-lint src --no-parallel

# Useful for:
# - Debugging (easier to follow in debugger)
# - Single-core machines
# - Very small codebases (under 100 files)
# - Profiling/benchmarking baseline
```

### Verbose Output

```bash
custom-biome-lint src -v
# May show thread pool info in future iterations
```

## Future Enhancements

### Thread Pool Tuning

```bash
# Planned: explicit thread count
custom-biome-lint src --threads 2   # Force 2 threads
custom-biome-lint src --threads 0   # Auto-detect (current default)
```

### Per-Rule Parallelization

Currently unsupported (and unlikely to help):
- Parsing already dominates, not per-rule logic
- Would require either:
  - Splitting file analysis into parallel stages (complex)
  - Threading within a single file (Rowan not optimized for this)

### Distributed Analysis

For huge codebases (100K+ files):
- Distribute files across multiple machines
- Aggregate results in CI
- (Far future, out of scope)

## Testing

### Unit Tests

```rust
#[test]
fn parallel_flag_overrides() {
    assert!(parse(&["--parallel"]).unwrap().parallel);
    assert!(!parse(&["--no-parallel"]).unwrap().parallel);
}
```

**Location:** `src/cli/args.rs::tests`

### Integration Tests

```bash
# Test 1: Sequential baseline
cargo run -- fixtures --no-parallel --no-cache

# Test 2: Parallel baseline  
cargo run -- fixtures --parallel --no-cache

# Test 3: Both with cache
cargo run -- fixtures --parallel
```

**Expected:** All produce identical violations, different runtimes.

### Benchmarking

```bash
# Build release binary
cargo build --release

# Baseline
time ./target/release/custom-biome-lint src --no-parallel --no-cache

# Parallel
time ./target/release/custom-biome-lint src --parallel --no-cache

# With cache
time ./target/release/custom-biome-lint src --parallel
```

## Design Decisions

### Why File-Level, Not Per-Rule?

**Per-rule parallelization** (hypothetical):
```rust
// Don't do this:
for file in files {
    rules.par_iter().map(|rule| rule.check(file)).collect()
}
```

**Problems:**
- Each file parsed N times (N = rule count)
- Parsing dominates, so per-rule parallelism is wasted
- Current single-pass design is better

### Why No Locking?

Each closure captures **immutable references only**:
- `file: PathBuf` (owned copy)
- `source: String` (owned copy)  
- `rules: &[&dyn Rule]` (immutable borrow)
- `result: AnalyzedFile` (fresh result)

No shared mutable state → **no locks needed**.

### Why Not Use Thread-Local Caches?

Biome's Rowan ASTs are already immutable. Thread-local caches would:
- Add complexity
- Reduce memory efficiency (duplicate ASTs)
- Provide minimal benefit

Better to keep it simple.

## Maintenance

### Adding New Rules

When you add a new rule, parallelization is **automatic**:
- Rule is added to registry
- All files analyze with new rule in parallel
- No code changes needed

### Debugging Parallel Code

If you see a race condition (likely in cache module):

1. **Run single-threaded:** `--no-parallel`
2. **Check thread-safety of custom code:**
   - All captured values must be `Send + Sync`
   - Or wrapped in thread-safe types (`Arc<Mutex<T>>`)

3. **Use thread-safe types carefully:**
   - `Arc` for shared ownership
   - `Mutex` for interior mutability
   - `RwLock` for read-heavy workloads

## References

- **Rayon docs:** https://docs.rs/rayon/
- **Rowan (immutable AST):** https://docs.rs/rowan/
- **Biome (parser crates):** crates.io/crates/biome_js_parser
