# Prompt for the custom-biome-lint repo session

Paste everything below into a fresh Claude Code session with the `custom-biome-lint` repo
checked out. Written from real integration testing against the `dashboard` monorepo (v0.1.1),
not speculation — every claim below was reproduced.

> **Historical record, not current status.** This is the prompt as written at
> the time. Bug 1 (suppression marker collision) shipped in v0.2.0. The "Open
> question" below on per-platform binaries has since been resolved — and
> resolved differently than its own recommendation: the package ships actual
> per-platform npm packages with real `os`/`cpu` guards on each one (see
> [DISTRIBUTION.md](DISTRIBUTION.md) and
> [PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md)), not a bare `os`/`cpu` guard bolted
> onto the old single build-on-install binary as that section suggested as an
> interim step. The verification checklist's old-marker grep is also
> point-in-time: run as written, it flags this file and
> [MIGRATION_NOTES.md](MIGRATION_NOTES.md) themselves, since both
> intentionally keep old-marker text as migration examples — the checklist
> was for the working tree at that moment, not a check to re-run today. Left
> unedited below for the record.

---

## Context

`custom-biome-lint` is being integrated into a consumer project (`dashboard`) as a vendored
Rust source tree, built via an explicit script rather than an npm `postinstall` (to avoid
requiring the Rust toolchain on every `yarn install`). During that integration, two real bugs
surfaced, plus one open design question. Fix the two bugs; the third item just needs a decision
recorded somewhere (e.g. an issue or a docs update), not necessarily code changes right now.

## Bug 1 (blocking, fix first): suppression syntax collides with Biome's own

replace all occurrence pattern from biome-ignore to custom-biome-ignore
means we have changed from biome-ignore to custom-biome-ignore for all type of suppression it should be fixed in tools as well like --write-fix

This is a hard, unavoidable collision as long as this tool's marker starts with `biome-ignore`.
The two tools are meant to run side by side (`docs/CI_CD_INTEGRATION.md`: "It runs alongside
Biome, not inside it" — "Each has its own exit code, and both must pass"), so this is not a
theoretical edge case, it breaks that stated design goal outright for any file using this
tool's suppressions.

### Fix: example rename the marker prefix

`biome-ignore-line` → `custom-biome-ignore-line`
`biome-ignore-next-line` → `custom-biome-ignore-next-line`

(No change needed to how it works otherwise — same rule-name-list syntax, same trailing vs.
own-line-above placement rules, same `--` justification handling, same JSX `{/* */}` wrapping.
Just the literal prefix string changes.)

### Where to change it

The two constants are the single source of truth:

```rust
// src/suppress/mod.rs
pub const IGNORE_LINE: &str = "biome-ignore-line";
pub const IGNORE_NEXT_LINE: &str = "biome-ignore-next-line";
```

Change these two literals and every call site that reads them should follow automatically — but
grep to be sure nothing else hardcodes the old strings separately. From a full-repo search on
v0.1.1, these also need the same update because they hardcode the literal strings directly
(not via the constants) — mostly in doc comments and test fixtures, but verify each:

- `src/suppress/mod.rs` — doc comments (lines ~46-47) and every unit test's literal source
  string (lines ~180, 188, 197, 203, 209, 215, 224, 231, 237, 243, 249 in the v0.1.1 tree —
  re-derive with `grep -n "biome-ignore" src/suppress/mod.rs`, don't trust these line numbers
  once the constants change and reformat the file)
- `src/fixer.rs` — doc comments (lines ~26, 28) and every unit test (multiple `assert!`/
  `assert_eq!` calls with literal old-prefix strings, including one that asserts the _absence_
  of the old prefix at line ~642: `!plan.source.contains("// biome-ignore")` — this one is
  subtle, it needs to assert the new prefix's absence, not just get skipped because it happens
  to still be "true" for a different reason)
- `README.md` — 8 occurrences, all in the "Suppressions" and "Adding suppressions
  automatically" sections; these are the user-facing docs, get them right
- `docs/RULES.md`, `docs/MIGRATION_NOTES.md`,   `docs/ADDING_A_RULE.md` (the single canonical "how to add a rule" guide;
  `docs/for-new-rule-addition.md` was merged into it),
  `docs/CI_CD_INTEGRATION.md`, `docs/INTEGRATION_GUIDE.md`, `docs/PACKAGE_JSON_SETUP.md`,
  `docs/TESTING.md`, `docs/ARCHITECTURE.md`, `docs/EXTRACT_TO_SEPARATE_REPO.md`
  — all reference the old syntax in examples
- `fixtures/no_native_map/suppressed.js`, `fixtures/no_arrow_function_create_selector/suppressed.js`,
  `fixtures/reselect_arity_match/suppressed.js` — these are read by the test suite at runtime
  (`cargo test`), not just documentation; must use the new marker or the suppression tests
  will start failing (correctly) once the parser only recognizes the new prefix
- `tests/integration.rs` — check for any literal old-prefix strings alongside the fixture-based
  tests

After the rename, run the full test suite (`cargo test` — should still be 152 tests) and
`cargo clippy --all-targets -- -D warnings` before considering this done. Then rebuild fixtures
verification: `./target/release/custom-biome-lint fixtures` should still report the same 11
errors across 6 files as before (the rename shouldn't change violation counts, only the
suppression-comment text).

### This is a breaking change — version and migration

Every existing suppression comment written with the old marker becomes invisible to the tool
the moment this ships — identical in kind to the "renaming a rule is breaking" note already in
`docs/EXTRACT_TO_SEPARATE_REPO.md`. Bump accordingly (at minimum a minor bump given pre-1.0
semver looseness, but call it out loudly in whatever changelog/release notes exist — this is
not a patch).

For existing consumers (the `dashboard` integration currently has exactly 8 suppression
comments written with the old marker, all `no-native-map` on `mapboxgl.Map` call sites — added
via `--write-fix` before this bug was found): the mechanical fix on the consumer side is a
straight find-and-replace of `biome-ignore-line` → `custom-biome-ignore-line` and
`biome-ignore-next-line` → `custom-biome-ignore-next-line` across the consumer's tracked
suppression comments (not a re-run of `--write-fix`, since the violations are already
suppressed under the old marker — a plain rename is correct and non-destructive). Worth adding
a line to `MIGRATION_NOTES.md`-equivalent docs for other consumers doing the same upgrade.

## Bug 2: incremental cache produces a false-negative "0 files checked" after a large-then-small run

**Reproduction (exact sequence that triggered it):**

```sh
# 1. A run against a large file set (a brace-expanded pattern listing ~1800 files worked
#    equally well as a plain full-tree glob — size of the pattern string doesn't matter,
#    what matters is the file *set* recorded in the cache).
./custom-biome-lint 'src/**/*.{js,jsx}'
# succeeds normally, "cache saved at .custom-biome-lint-cache"

# 2. Immediately after, a run against a small, different file set:
./custom-biome-lint '{src/App.jsx,src/auth/azure.js,src/auth/okta.js}' -vv
```

Step 2's `-vv` output correctly shows `discovered 3 file(s) from 4730 considered across 830
director(ies)` — file _discovery_ is correct — but then reports:

```text
[v] cache: 1869 file(s) cached, 0 hit(s)
✔ No violations found (0 files checked in 58ms)
```

**0 files checked, despite 3 being correctly discovered.** This is a silent false-negative: the
tool exits 0 (success) having actually linted nothing, which for a correctness gate is worse
than a slow or crashing run — a CI job or pre-push hook reading only the exit code sees a clean
pass that never happened.

Deleting `.custom-biome-lint-cache` and re-running the exact same small-set command
immediately fixes it (`3 file(s) cached, 3 hit(s)`, `3 files checked` — correct). Adding
`--no-cache` to either run also avoids the issue reliably across every repro attempt (tested
~5 times, large→small, small→large, same-set-twice — only large-set-then-different-small-set
reproduced it).

**Not yet root-caused** — that's the ask. Start in `src/cache/mod.rs`
(`CacheManager`, `CacheEntry`, `hash_content`). Hypothesis worth checking first: something in
how a "hit" is counted vs. how the _checked_ count is derived from the file list after cache
filtering — the discovered set (3) and the "files checked" count (0) diverge, which suggests
the post-cache-filter file list handed to the actual lint pass is empty even though the
pre-filter discovered list wasn't, which points at whatever function intersects "discovered
files" with "cache misses" (or inverts a hit/miss check) rather than at the hashing or
persistence layer itself.

Once fixed, add a regression test that reproduces this exact sequence (run 1 with a large
synthetic file set, run 2 with a small disjoint-but-overlapping set, assert run 2's "files
checked" count matches its discovered count) — the existing test suite apparently didn't
exercise "cache populated by one run, consulted by a differently-scoped run," which is exactly
the CI/pre-push usage pattern (full-tree job and changed-files job sharing a cache dir would hit
this).

Until fixed, consumers should be told (README caveat, or a runtime warning when cache size
diverges sharply from the current discovered set?) that `--no-cache` is the safe choice for any
usage where the file-set scope varies between runs — which describes a "changed files only" CI
check specifically, one of the two documented primary use cases in `docs/CI_CD_INTEGRATION.md`.

## Open question (no code change needed yet, just a decision): per-platform binaries

Current `package.json` ships one prebuild-free path (`postinstall: cargo build --release`),
which is honest but means every consumer needs the Rust toolchain to `yarn add -D
custom-biome-lint`, which mostly defeats the point of publishing to npm at all (a
Rust-toolchain-having consumer could just vendor the source, which is what the `dashboard`
integration is doing specifically to sidestep this).

`docs/PUBLISH_TO_NPM.md` already lays out the options table (build-on-install / os-cpu-guard /
per-platform-optionalDependencies / cargo-dist) — recommendation, for whenever this gets
picked up: **`os`/`cpu` guards are cheap and worth adding regardless of what else happens**
(turns a silent wrong-platform runtime failure into a clear install-time error, no design work
required). Per-platform `optionalDependencies` packages (the esbuild/swc pattern) is the correct
answer if the goal is "any team can `yarn add -D` this with zero Rust required" — but it's real,
ongoing CI-matrix work, not a quick fix, so treat it as a separate, larger piece of work rather
than bundling it into the suppression-syntax fix above.

## Verification checklist before calling this done

- [ ] `cargo test` — 152 tests passing (same count as before; a lower count means a fixture or
      test got deleted rather than updated)
- [ ] `cargo fmt --all -- --check` — no diff
- [ ] `cargo clippy --all-targets -- -D warnings` — no warnings
- [ ] `./target/release/custom-biome-lint fixtures` — still 11 errors across 6 files
- [ ] Grep confirms zero remaining occurrences of the bare old prefix outside of
      changelog/migration-notes history: `grep -rn "biome-ignore-line\|biome-ignore-next-line" . --include="*.rs" --include="*.md" --include="*.js" | grep -v target/`
- [ ] The cache regression test (new) passes, and manually repeat the original repro sequence
      once more against a real checkout to confirm it no longer reproduces
- [ ] Version bumped in both `Cargo.toml` and `package.json` (per `docs/PUBLISH_TO_NPM.md`'s
      existing warning that these drift silently), `Cargo.lock` refreshed, both committed together
