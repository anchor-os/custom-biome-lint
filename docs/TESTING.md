# Testing

From fastest to most realistic:

1. `cargo test` — unit, integration and doctests
2. `cargo fmt --all -- --check` — formatting
3. `cargo clippy --all-targets` — lint the linter
4. `cargo audit` / `cargo deny check` — dependency advisories, licenses, sources
5. Run the binary against `fixtures/` — known-good end-to-end check
6. Run the binary against the real dashboard `src/` tree — parity check

Plus a one-off portability check documented at the end.

## 1. `cargo test`

```sh
cargo test
```

Expected: **137 passing, 0 failing**, in four suites.

```text
Running unittests src/lib.rs
running 84 tests
test result: ok. 84 passed

Running unittests src/bin/custom-biome-lint.rs
running 0 tests
test result: ok. 0 passed

Running tests/integration.rs
running 52 tests
test result: ok. 52 passed

Doc-tests custom_biome_lint
running 1 test
test result: ok. 1 passed
```

The binary suite reporting 0 tests is expected — `src/bin/custom-biome-lint.rs` is
7 lines that delegate to `cli::run()`, so everything is tested through the
library.

### Unit tests (84, in `src/`)

Colocated `#[cfg(test)]` modules testing components in isolation.

| Module | Count | What it covers |
| --- | --- | --- |
| `analyzer::file_matcher` | 8 | Glob semantics: `*` staying within a segment, `**` spanning directories, `?`, brace expansion, extension collection from alternatives, `root_dir()` stopping at the first wildcard |
| `autofix` | 13 | A single fix applied, a violation with no `Fix` left as skipped rather than dropped, overlapping fixes only applying the first, a fix that would break parsing rejected before writing, invalid fix ranges (reversed, past the end, splitting a UTF-8 code point) rejected without panicking, a file changed on disk since analysis skipped rather than rewritten at stale offsets, atomic writes replacing target content, the original file mode surviving the rename, exclusive temp-file creation refusing a pre-existing symlink, dry run vs write behaviour |
| `cache` | 9 | Content-hash cache creation, marking/saving, content changes invalidating regardless of mtime, identical content staying valid after a rewrite, cache-key (rule set + version) changes invalidating, disk round-trip, corrupted-cache recovery, old mtime-format entries silently ignored |
| `cli::args` | 20 | Quiet defaults, repeated `-v`, verbosity saturating at 3, clustered short flags (`-vd`), positional pattern, rejection of unknown flags and duplicate positionals, `--write-fix`/`--auto-fix` defaulting to writing, `--dry-run` requiring one of them, `--write-fix` and `--auto-fix` rejected together, `--format` parsing and its rejection alongside `--write-fix`/`--auto-fix` |
| `config::package_config` | 7 | Missing `package.json`, legacy array form, object form with `off`/`warn`/`error`, malformed entries warning and being skipped |
| `diagnostics::formatter` | 3 | JSON output carries every field, omits clean files, is stable for a clean run |
| `suppress` | 10 | Same-line and next-line markers, multiple comma-separated rules, `--` justification suffix, block-comment and JSX brace forms, marker inside a string literal being ignored, bare marker suppressing every rule, `append_at` placement for merges |
| `fixer` | 14 | Trailing vs own-line placement, indentation, several rules sharing one comment, extending an existing comment, justification surviving a merge, JSX brace form on its own line, JSX attributes treated as code, template-literal and parse-error refusals, CRLF and missing-final-newline round-trips, idempotency, the lexer's multi-line and regex-resync behaviour |

Run one group:

```sh
cargo test --lib suppress
cargo test --lib file_matcher
```

### Integration tests (52, in `tests/integration.rs`)

These drive the public API — mostly `lint_source`, which runs the full
parse → check → suppression-filter pipeline, the same path the CLI uses. The
`cli_behavior` module instead runs the built binary as a subprocess, for
behaviour that only exists at the `cli::run()` layer, and `semantic_model`
drives `FileContext::semantic()` directly.

| Module | Count | What it covers |
| --- | --- | --- |
| `no_native_map` | 7 | Native `Map` reported; Immutable named import, namespace-plus-destructure, and `require` alias all allowed; `Map` from an unrelated module still reported; suppressions work; edge cases produce exactly the documented violations |
| `reselect_arity_match` | 5 | Mismatched arity reported, matching arity allowed, member-expression callee (`reselect.createSelector`) checked, suppressions work, edge cases flag only the namespaced mismatch |
| `no_arrow_function_create_selector` | 5 | Wrapped `createSelector` reported with a fix attached, direct call and `make*` factory allowed, suppressions work, edge cases flag only the non-factory `make`-prefixed name, an `async` wrapper is reported but left without a fix |
| `semantic_model` | 20 | Basic declarations, function parameters, nested-scope shadowing, all four import forms and their source/imported/local fields, a parameter and a local redeclaration each shadowing a same-named import, object/array destructuring (including a computed key as a reference), arrow parameters, block scope, catch scope, a switch statement's cases sharing one block scope, `var` hoisting out of a nested block, a `let` scoped to a `for` loop head, the scope parent-chain hierarchy |
| `patterns` | 4 | Bare directory expands to a brace glob, bare directory discovers every fixture, explicit glob passed through unchanged, `node_modules` never walked |
| `cli_behavior` | 4 | `--format json` still emits a document when every rule is disabled; `--auto-fix` unwraps the arrow and relints clean; `--auto-fix --dry-run` leaves the file untouched; `--write-fix` and `--auto-fix` together is rejected |
| `config` | 3 | `ignoreBiomeExtensionRules` filters rules out; missing `package.json` enables everything; `warn` severity reports without disabling |
| `extensions` | 1 | An unsupported extension yields no violations |
| top level | 3 | Registry exposes all three rules; default pattern covers `.js` and `.jsx`; every rule has fixtures for all four cases |

The four `patterns` tests are regressions for a real bug: the bare-directory
shorthand was producing `fixtures/**/*.js,jsx` instead of
`fixtures/**/*.{js,jsx}` because of a missing brace escape in a `format!` string.
That silently matched **nothing**, so the tool reported a clean run — the worst
failure mode for a linter. Do not delete these tests.

`every_rule_has_fixtures_for_all_four_cases` is a guard that fails if a new rule
is registered without `valid.js`, `invalid.js`, `suppressed.js` and
`edge-cases.js`.

### Doctest (1)

The example on `lint_source` in `src/lib.rs`. It is `no_run` — compiled and
type-checked but not executed — so it guarantees the documented public API keeps
compiling.

```sh
cargo test --doc
```

## 2. Formatting

```sh
cargo fmt --all -- --check
```

Expected: **no diff**. `rustfmt.toml` pins `edition = "2021"`; run `cargo fmt
--all` (without `-- --check`) to apply the fix rather than just report it.

## 3. Clippy

```sh
cargo clippy --all-targets
```

Expected: **no warnings**. `--all-targets` matters — it covers the lib, the
binary, and the test targets; a bare `cargo clippy` skips tests.

To hold the line in CI, escalate warnings to errors:

```sh
cargo clippy --all-targets -- -D warnings
```

## 4. Supply chain: `cargo audit` / `cargo deny`

```sh
cargo audit
cargo deny check
```

Both read from configuration already in the repo rather than needing flags:
`.cargo/audit.toml` for `cargo audit`, `deny.toml` for `cargo deny`.

Expected: **no errors** from either. `cargo audit` checks `Cargo.lock` against
the RUSTSEC advisory database; `cargo deny check` additionally verifies every
dependency's license is on the allow list in `deny.toml`, and that nothing
resolves from an unexpected registry or git source. `deny.toml`'s
`multiple-versions = "warn"` means duplicate transitive versions (e.g. two
versions of `syn`) print as noise, not failures — that's expected for a
dependency graph this size and not itself a problem to fix.

Both tools are installed from prebuilt binaries in CI (`taiki-e/install-action`)
rather than compiled from source, and are not part of what ships in the
package — they only run as CI/dev checks against `Cargo.lock`.

## 5. Against the fixtures

The fastest end-to-end check with a known-exact expected result.

```sh
cargo build --release
./target/release/custom-biome-lint fixtures
```

Expected output — **12 errors in 6 files**, exit code 1:

```text
fixtures/no_arrow_function_create_selector/edge-cases.js
  20:23  error  Avoid wrapping createSelector in an arrow function for "makeup". ...  no-arrow-function-create-selector

fixtures/no_arrow_function_create_selector/invalid.js
  7:35   error  Avoid wrapping createSelector in an arrow function for "selectVisibleUsers". ...  no-arrow-function-create-selector
  12:32  error  Avoid wrapping createSelector in an arrow function for "selectFirstUser". ...     no-arrow-function-create-selector

fixtures/no_native_map/edge-cases.js
  7:26   error  Use Immutable.js Map instead of native Map.  no-native-map
  12:25  error  Use Immutable.js Map instead of native Map.  no-native-map
  13:14  error  Use Immutable.js Map instead of native Map.  no-native-map

fixtures/no_native_map/invalid.js
  3:26  error  Use Immutable.js Map instead of native Map.  no-native-map
  6:22  error  Use Immutable.js Map instead of native Map.  no-native-map

fixtures/reselect_arity_match/edge-cases.js
  20:86  error  createSelector expects 2 parameter(s) in the result function, but found 1.  reselect-arity-match

fixtures/reselect_arity_match/invalid.js
  7:77   error  createSelector expects 2 parameter(s) in the result function, but found 1.  reselect-arity-match
  10:64  error  createSelector expects 1 parameter(s) in the result function, but found 2.  reselect-arity-match
  13:76  error  createSelector expects 2 parameter(s) in the result function, but found 1.  reselect-arity-match

✖ 12 errors in 6 files
```

`invalid.js` and `edge-cases.js` are the only files that ever appear — each
`edge-cases.js` violation is a pinned, deliberate count for a documented
boundary behavior (see [RULES.md](RULES.md)), not a bug. If `valid.js` or
`suppressed.js` shows up, or the counts drift from the above, something is
broken — either a rule became over-eager or suppression parsing regressed.

Note this also exercises the bare-directory shorthand: `fixtures` expands to
`fixtures/**/*.{js,jsx}`.

Verify the exit code, since CI depends on it:

```sh
./target/release/custom-biome-lint fixtures; echo "exit=$?"   # exit=1
```

| Code | Meaning |
| --- | --- |
| 0 | No violations |
| 1 | Violations found |
| 2 | Bad usage, or the pattern's root directory does not exist |

## 6. Against the real dashboard tree

Run from `UI/dashboard` (the directory containing `src/`), pointing at the built
binary:

```sh
./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}'
```

Quote the glob so the shell does not expand it first.

Expected: **8 errors in 8 files**, all `no-native-map`.

```
src/components/AddExternalEvents/ExternalEventForm.jsx
  173:30  error  Use Immutable.js Map instead of native Map.  no-native-map
...
✖ 8 errors in 8 files
```

### Interpreting that output

All 8 findings are **expected and pre-existing**. Each one:

- is the `new mapboxgl.Map()` false-positive class — faithful to the original
  ESLint rule, which could not distinguish a member-expression `.Map` from the
  global `Map` (see [RULES.md](RULES.md))
- sits exactly one line below an existing
  `// eslint-disable-next-line customPlugin/no-native-map` comment

That second point is the parity evidence: 8 findings, 8 pre-existing
suppressions, each adjacent. To re-verify it yourself:

```sh
grep -rn "no-native-map" src --include="*.js" --include="*.jsx"
```

Eight hits, whose line numbers are each one less than the corresponding reported
line. Confirming this correspondence is the actual acceptance test for the port —
it demonstrates the rule reproduces ESLint's behaviour rather than approximating
it.

The other two rules report **0 violations** across the codebase's 141
`createSelector` call sites, which is also expected: the ESLint rules have kept
those patterns clean.

### After migrating the suppression comments

Once the 8 comments are translated to `// biome-ignore-next-line no-native-map`
(see [MIGRATION_NOTES.md](MIGRATION_NOTES.md)), the same command should report:

```
✔ No violations found
```

and exit 0. That is the state CI should enforce.

### Useful verbosity while investigating

```sh
# What config was found, which rules are enabled/skipped, resolved pattern
./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}' -v

# Brace expansion, walk root, directories scanned/skipped, file counts
./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}' -vv

# Per-file: rules run, violation count, line count
./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}' -vvv

# Everything, with source locations on each log line
./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}' -d --trace
```

Diagnostics go to **stdout**; all logging goes to **stderr**. So this captures
just the report:

```sh
./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}' -vv > report.txt
```

and this captures just the logs:

```sh
./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}' -vv 2> debug.log
```

If a file fails to parse, the tool warns on stderr rather than skipping silently
— an unparseable file would otherwise look identical to a clean one in CI.

## 7. Portability check

The package is meant to be liftable into its own repository, published to npm, or
vendored as a submodule with no edits. Verifying that means building it somewhere
it cannot possibly see the surrounding repo.

How it was done:

```sh
# Copy everything except build output to a location outside the repository
rsync -a --exclude 'target/' \
  /path/to/UI/dashboard/custom-biome-lint/ \
  /tmp/portability-check/custom-biome-lint/

cd /tmp/portability-check/custom-biome-lint
cargo build --release
cargo test
./target/release/custom-biome-lint fixtures
```

Result: clean build from scratch in **30.73s**, all 42 tests passing, and the
fixture run produced the same 7 errors in 3 files.

Excluding `target/` is the point — it forces a genuine cold build and proves no
stale artifact was carrying a dependency the manifest does not declare.

What this establishes:

- No path dependencies or workspace inheritance from the dashboard repo
- The committed `Cargo.lock` reproduces the pinned `0.5.7` Biome graph correctly
  (see the version-pinning section in [ARCHITECTURE.md](ARCHITECTURE.md))
- Nothing reads from the surrounding repository. The only external input is the
  `package.json` discovered at runtime, and its absence is handled — the tool
  runs with all rules enabled, which is why the fixture run works outside a JS
  project

Re-run this check after any change to `Cargo.toml`, and especially before
extracting the package to its own repository.

## Full pre-commit sweep

```sh
cargo fmt --all -- --check \
  && cargo test \
  && cargo clippy --all-targets -- -D warnings \
  && cargo audit \
  && cargo deny check \
  && cargo build --release \
  && (
    set +e
    ./target/release/custom-biome-lint fixtures
    code=$?
    set -e
    test "$code" -eq 1
  )
```

The fixtures command is expected to exit 1 (the fixtures contain deliberate
violations), which would otherwise break the `&&` chain — `test "$code" -eq 1`
converts that specific, expected status back into success so the whole
sweep's own exit code reports whether anything is *actually* wrong. `code`
rather than `status`, since `status` is a read-only special variable in zsh —
assigning to it fails outright if this is pasted into a zsh shell, which is
the default on macOS.

`cargo audit` and `cargo deny` require the respective binaries installed
locally (`cargo install cargo-audit cargo-deny`, or `brew install cargo-audit
cargo-deny`) — CI installs them fresh on every run instead, so a missing local
install only affects this sweep, not the pipeline.
