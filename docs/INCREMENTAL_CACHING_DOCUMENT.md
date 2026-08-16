# Incremental Caching

## Overview

The `custom-biome-lint` tool implements **incremental caching** to skip re-analyzing unchanged files on subsequent runs. See `docs/BENCHMARKING.md` for current, re-runnable speedup numbers rather than a hardcoded figure here.

**Enabled by default.** Disable with `--no-cache` if needed.

## Cache Strategy

### What Gets Cached?

Files with **zero violations** are cached. Why?
- If a file had violations last run and still does now, re-running it is cheap (violations already reported)
- If a file had zero violations and still does, we can safely skip it
- If a file had violations but now has zero, we MUST run it (to report the fix)

### Cache Keys

Each cached file stores two pieces of data:

| Key | Value | Purpose |
|-----|-------|---------|
| `content_hash` | Hash of the file's own bytes | Detect file changes |
| `cache_key` | Hash of enabled rule names + tool version | Detect rule/version changes |

**Not mtime.** An earlier version of this cache used file mtime instead of a
content hash. That looks free — no need to read the file to check it — but a
fresh checkout (the common case in CI) gives every file a new mtime
regardless of whether its content changed, which defeats the cache on
exactly the runs where it matters most. Content hashing costs reading every
candidate file every run, but that read was already happening for any file
the tool needed to actually check; what the cache skips is the *parse and
rule execution* that follows, which is where ~70% of run time goes (see
docs/ARCHITECTURE.md), not the read itself.

### Invalidation

Cache is **automatically invalidated** when:

1. **File content changes** (content hash changes)
   ```
   Cached content_hash: 8e89937c181c706d
   Current content_hash: a41f02cd99b13e02
   → Cache invalid, re-analyze
   ```
   A file rewritten with byte-for-byte identical content — e.g. a fresh git
   checkout, which bumps mtime but not content — keeps the same hash and
   **stays cached.** This is the exact case mtime-based caching got wrong.

2. **Rules or tool version change** (cache_key changes)
   ```
   Cached cache_key: 8e89937c181c706d (3 rules enabled, tool v0.1.0)
   Current cache_key: 3f1a9c0b2e77d451 (4 rules enabled, or a rule's logic
                                          changed under a new tool version)
   → Cache invalid, re-analyze all files
   ```
   The tool's own version is folded into this key because a rule's detection
   logic — or the set of enabled rules — can change what a given file's
   content produces, even though the file itself never changed.

## Cache Structure

### Location

```
.custom-biome-lint-cache/
└── cache.json
```

At the project root, **not** under `node_modules/`: this tool has no npm
dependencies of its own, so nesting its cache under `node_modules/` implied a
dependency relationship that doesn't exist. `.gitignore` excludes it.

### Format

```json
{
  "version": "2",
  "entries": {
    "/path/to/file.js": {
      "content_hash": "8e89937c181c706d",
      "cache_key": "3f1a9c0b2e77d451"
    },
    "/path/to/other.jsx": {
      "content_hash": "a41f02cd99b13e02",
      "cache_key": "3f1a9c0b2e77d451"
    }
  }
}
```

**Schema:**
- `version`: Format version (`"2"` — bumped from `"1"` when the cache moved
  from mtime to content hashing; the two are not compatible, so an old
  `cache.json` is simply ignored on load rather than misread)
- `entries`: Map of file path → cache metadata
- `content_hash`: Hash of the file's content at the time it was last found clean
- `cache_key`: Hash of enabled rule names + tool version

## Implementation

### Code Structure

**`src/cache/mod.rs`** - `CacheManager`:

```rust
pub struct CacheManager {
    cache_dir: PathBuf,
    cache_data: HashMap<String, CacheEntry>,
}

/// Hashes file content for cache-key purposes; not cryptographic.
pub fn hash_content(source: &str) -> String

impl CacheManager {
    pub fn new(cwd: &Path) -> Result<Self, String>
    pub fn load(&mut self) -> Result<(), String>
    pub fn is_valid(&self, path: &Path, content_hash: &str, cache_key: &str) -> bool
    pub fn mark_valid(&mut self, path: &Path, content_hash: &str, cache_key: &str)
    pub fn save(&self) -> Result<(), String>
}
```

`mark_valid` no longer returns `Result`: computing a content hash from a
string already in memory can't fail the way reading file metadata could.

### Analysis Flow

```
1. Initialize cache
   ├─ Create CacheManager
   ├─ Load existing cache.json (if present)
   └─ Compute cache_key (compute_cache_key in cli::mod, from enabled rules + tool version)

2. Analyze files
   ├─ For each file:
   │  ├─ Read its content (there is no cheaper check available -- see above)
   │  ├─ Hash the content
   │  ├─ Check is_valid(file, content_hash, cache_key)
   │  ├─ If cached: skip parsing and rule execution
   │  └─ If not cached: analyze; mark_valid() later if the result is clean
   └─ Collect violations

3. Save cache
   ├─ Serialize cache_data to JSON
   ├─ Write to cache.json
   └─ (Errors are non-fatal)
```

### Cache Key Computation

`compute_cache_key` (in `src/cli/mod.rs`) hashes the sorted, joined enabled
rule names together with the tool's own `CARGO_PKG_VERSION`, using
`std::collections::hash_map::DefaultHasher`:

```rust
fn compute_cache_key(rules: &[&dyn Rule]) -> String {
    let mut rule_names: Vec<&str> = rules.iter().map(|r| r.name()).collect();
    rule_names.sort_unstable();

    let mut hasher = DefaultHasher::new();
    rule_names.join(",").hash(&mut hasher);
    VERSION.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}
```

Sorting makes the hash independent of registration order. Joining with a
separator (rather than plain concatenation) means `["ab", "c"]` and `["a",
"bc"]` can't collide. This replaced an earlier version that hashed
`rule_names.len()` — the length of the concatenated names — which is a real
collision hazard (two different rule sets of equal total name length were
indistinguishable) rather than a simplification; see the git history for
that fix.

## Performance

### Measured Results

See `docs/BENCHMARKING.md` for a re-runnable harness and current numbers —
this section previously hardcoded a one-off measurement from the mtime-based
cache, which is exactly the kind of number that silently rots. The
qualitative trade-off from switching to content hashing: warm runs no longer
skip *reading* an unchanged file (mtime-based caching could), but they still
skip the parse and rule execution that follows, which is the dominant cost
(see docs/ARCHITECTURE.md's ~70%-parsing measurement) — so the cache's actual
purpose is intact even though its cheapest possible case got slightly more
expensive.

### When Cache Helps Most

| Scenario | Benefit |
|----------|---------|
| **CI on unchanged files** | 56-63x (warm run) |
| **Local dev** (after first run) | Same as CI |
| **Incremental builds** | High (most files unchanged) |
| **Clean checkout** | None (cold run) |
| **After major refactor** | None initially, then high |

## CLI Interface

### Default Behavior

```bash
# Cache enabled by default
custom-biome-lint src
# Equivalent to:
custom-biome-lint src  # (--no-cache not specified)
```

### Disable Caching

```bash
# Force re-analysis of all files
custom-biome-lint src --no-cache

# Useful for:
# - Debugging (avoid cached results)
# - CI jobs that should be clean
# - Sanity checks (verify cache correctness)
```

### Verbose Output

```bash
custom-biome-lint src -v
# Output (in verbose mode):
# "cache: 500 file(s) cached total, 450 skipped this run (already valid), 50 newly marked clean this run"
```

Three distinct counts, not one "hit(s)" figure — an earlier version of this
line conflated "files this run freshly analyzed and found clean" (labeled
"hits", confusingly) with actual cache hits (files the cache already had, so
this run never even re-read their AST). See "A `0` in `filesChecked` is not
necessarily a bug" below for why that distinction matters.

### A `0` in `filesChecked` is not necessarily a bug

Running against a narrower pattern shortly after a broader one that already
covered the same files is a real, common sequence -- e.g. a full-tree CI job
followed by a changed-files-only job sharing the same cache directory. If
every file the narrower run discovers is unchanged since the broader run
found it clean, the cache correctly skips re-analyzing all of them:

```bash
custom-biome-lint 'src/**/*.{js,jsx}'                              # large run, caches everything clean
custom-biome-lint '{src/Example.jsx,src/auth/example.js,src/sagas/example.js}' # small, overlapping run
# ✔ No violations found (0 files checked, 3 skipped via cache in 5ms)
```

`filesChecked: 0` here does **not** mean discovery found nothing, or that the
run silently did nothing useful -- it means every discovered file was
already known clean and unchanged, so re-deriving that fact would have been
wasted work. That is the cache doing exactly its job. What used to make this
look like a false negative was purely a reporting gap: the summary line
(and the JSON `summary` object) had no way to distinguish "0 discovered" from
"0 needed re-checking, N already cache-valid" -- both printed the identical
`0 files checked`. The fix was adding `filesCacheSkipped` (and the
`, N skipped via cache` clause in the text summary, shown only when nonzero
so the common no-cache case reads exactly as before) — not changing when the
cache is allowed to skip a file. Making the cache re-check unchanged files
just because the pattern scope changed would defeat the point of a
content-hash cache.

If a workflow genuinely needs every invocation to fully re-analyze its own
file set regardless of what a previous, differently-scoped run already
cached — e.g. a from-scratch CI job that must never trust another job's
cache directory — pass `--no-cache`.

## Testing

### Unit Tests

**Location:** `src/cache/mod.rs::tests`

```rust
#[test]
fn cache_marks_and_saves_files()
#[test]
fn cache_detects_content_changes_even_with_an_unchanged_mtime()
#[test]
fn identical_content_stays_valid_even_if_rewritten()
#[test]
fn cache_detects_rule_hash_changes()
#[test]
fn cache_loads_from_disk()
#[test]
fn corrupted_cache_is_recovered()
#[test]
fn old_mtime_format_cache_is_silently_ignored_not_misread()
```

**All tests pass.** Cache is:
- ✓ Persistent (survives session restarts)
- ✓ Invalidated correctly on content or cache-key changes
- ✓ **Not** falsely invalidated by a fresh checkout that only bumps mtime
- ✓ Recoverable (corrupted cache, or an old mtime-format cache, don't crash
  or get misread)

### Integration Testing

```bash
# Test 1: Cold run (cache cleared)
rm -rf .custom-biome-lint-cache
time custom-biome-lint src --no-parallel

# Test 2: Warm run (cached)
time custom-biome-lint src --no-parallel
# Expected: dramatically faster -- see docs/BENCHMARKING.md for current numbers

# Test 3: Change one file's content
echo "changed" >> src/somefile.js
time custom-biome-lint src --no-parallel
# Expected: only that file's rules re-run

# Test 4: Touch a file without changing its content (simulates a checkout)
touch src/somefile.js
time custom-biome-lint src --no-parallel
# Expected: still a cache hit -- this is the case mtime-based caching got wrong

# Test 5: Modify a rule or bump the tool version
# Expected: re-analyze all files (cache_key changed)

# Test 6: Disable cache
time custom-biome-lint src --no-cache
# Expected: same as a cold run
```

## Design Decisions

### Why content hash, not mtime + rule_hash?

**Alternatives considered:**

| Approach | Pros | Cons |
|----------|------|------|
| **mtime + rule_hash** (original) | No need to read the file to check it | A fresh checkout bumps every file's mtime regardless of content, defeating the cache on exactly the runs (CI) where it matters most |
| **Content hash + cache_key** (current) | Correct across checkouts; only reads what was going to be read anyway | Can't skip the file read itself, only the parse + rule execution after it |
| **Git commit hash** | Works with git workflows | Fails outside git repos; this tool is deliberately usable standalone (e.g. against `fixtures/`) |
| **Timestamps + checksums** | Very robust | Overcomplicated for what this cache needs |

**We chose content hash + cache_key because:**
- The dominant cost this cache exists to avoid is parsing and rule
  execution (~70% of run time, see docs/ARCHITECTURE.md), not the file
  read — so trading away the "skip the read too" property of mtime-based
  caching for correctness across checkouts is the right trade
- `DefaultHasher` (already used elsewhere in this codebase) needs no new
  dependency and is fast enough for content already in memory
- Folding the tool's own version into the cache key means an upgrade that
  changes rule behavior can't be served stale results from before the
  upgrade

### Why JSON (not SQLite/TOML/YAML)?

**JSON chosen because:**
- ✓ Builtin serde support (no extra crate)
- ✓ Human-readable (debugging)
- ✓ Version-aware (`"version": "1"` field)
- ✓ No dependencies beyond serde_json (already used)

**SQLite alternative:**
- ✗ Overkill for simple key-value data
- ✗ Binary format (harder to debug)
- ✗ Extra dependency

### Why Skip Files with Violations?

Question: Why not cache even files with violations?

**Answer:** Safety.
- If a file had violations and now has zero violations, we MUST report the fix
- Caching violations requires careful tracking:
  - Which violations were reported?
  - What if rule behavior changed (new false positive)?
  - Better to be conservative

**Real impact:** Files with violations change less frequently than files with zero violations in typical codebases (most files are "clean").

### Why `.custom-biome-lint-cache/` at the project root?

**Alternatives:**
- `node_modules/.custom-biome-lint-cache/` (original choice)
- `~/.cache/custom-biome-lint/` (home directory)
- Project-relative `.biome-cache/`

**We moved away from `node_modules/` because:**
- ✗ This tool has no npm dependencies of its own, so nesting its cache
  under `node_modules/` implied a dependency relationship that doesn't exist
- ✓ A plain `.custom-biome-lint-cache/` at the project root is still
  gitignored, still ephemeral, and doesn't need `node_modules/` to exist at
  all — this tool is deliberately usable in a plain Rust checkout with no
  npm involved (e.g. running it against `fixtures/`)

## Error Handling

### Corrupted Cache

```
Scenario: cache.json has invalid JSON
Response: 
  1. Log warning (if verbose)
  2. Clear cache (return empty CacheManager)
  3. Continue (re-analyze all files)
  4. Write new cache.json
```

**Result:** Corrupted cache is **non-fatal**. The tool recovers and rebuilds cache.

An old, pre-content-hash cache.json (with `mtime`/`rule_hash` fields instead
of `content_hash`/`cache_key`) is handled the same way as a missing cache:
`load()` skips any entry missing the fields it expects, so every file simply
misses the cache once and gets re-cached in the new format — no corruption,
no crash, no explicit migration step needed.

### Missing Cache Directory

```
Scenario: .custom-biome-lint-cache/ doesn't exist yet
Response:
  1. CacheManager::new() succeeds (returns empty cache)
  2. analyze() finds nothing cached (no entries loaded)
  3. After analysis, save() creates .custom-biome-lint-cache/
```

**Result:** Cache initializes lazily on first analysis.

### Permission Denied

```
Scenario: Can't write to .custom-biome-lint-cache/
Response:
  1. Log warning (non-fatal)
  2. Continue without persisting cache
  3. Next run re-analyzes all files
```

**Result:** Inaccessible cache is **non-fatal**. Tool still works, just slower.

## Future Enhancements

### Distributed Cache

For monorepos / CI farms:
```bash
# Planned (future):
custom-biome-lint src --cache-dir /shared/ci-cache
# Multiple CI workers share cache (with locking)
```

### Cache Statistics

```bash
# Planned (future):
custom-biome-lint src -vv
# Output cache stats:
# - Cache size: 50MB
# - Cache hits: 4000/4100
# - Cache hit rate: 97.6%
# - Time saved: 28.3 seconds
```

Partially done: `-v` now reports an accurate split of cached/skipped/newly-cached
counts (see "Verbose Output" above), and both the text summary and
`--format json`'s `filesCacheSkipped` field expose the raw skip count outside
of `-v` too. Cache size, an explicit hit-rate percentage, and an estimated
time-saved figure are still unimplemented.

### Smart Invalidation

Currently: all files re-analyzed if any rule changes.

Planned: per-rule caching
```
Rule A change → re-analyze only files cached for rules that changed
(requires tracking which rules cached each file)
```

## Maintenance

### Clearing Cache

Users can always clear cache:
```bash
rm -rf .custom-biome-lint-cache
```

Or disable it:
```bash
custom-biome-lint src --no-cache
```

### Debugging Cache Issues

1. **Check cache exists:**
   ```bash
   ls -la .custom-biome-lint-cache/cache.json
   ```

2. **Inspect cache content:**
   ```bash
   cat .custom-biome-lint-cache/cache.json | jq
   ```

3. **Force re-analysis:**
   ```bash
   custom-biome-lint src --no-cache
   # Compare output with cached run
   ```

4. **Verify cache correctness:**
   ```bash
   # Both should produce identical violations
   custom-biome-lint src --no-cache
   custom-biome-lint src  # (with cache)
   ```

## References

- **Cache implementation:** `src/cache/mod.rs`
- **Integration point:** `src/cli/mod.rs` (analyze loop)
- **Test suite:** `src/cache/mod.rs::tests`
