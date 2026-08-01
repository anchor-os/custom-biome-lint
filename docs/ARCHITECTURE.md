# Architecture

How `custom-biome-lint` is put together, and why each of the non-obvious
decisions was made. If you are picking this up cold, read this file first and
then [RULES.md](RULES.md).

## What this tool is

A standalone Rust binary that lints JavaScript/JSX for three Redux/Reselect/
Immutable patterns that Biome has no equivalent for. It exists because the
dashboard's migration from ESLint to Biome 2.5.5 would otherwise silently drop
three custom ESLint rules that guard against real, silent bugs.

It is **not** a Biome plugin. Biome 2.5.5 has no plugin API, so this runs as an
independent check alongside `biome check`. See
[CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md) for how that is intended to be
wired up, and for the forward-looking note on what changes if Biome ever ships a
plugin API.

## Design in one paragraph

The CLI resolves a glob pattern, walks the filesystem to find matching files,
and for each file parses it **exactly once** into a `FileContext`. Every enabled
rule then inspects that one shared syntax tree. The runner — not the individual
rules — decides which rules apply to a file (by extension) and drops any
violation that a suppression comment covers. Rules therefore contain nothing but
detection logic.

```
main() -> cli::run()
            |
            |-- PackageConfig::load()       find nearest package.json, read ignore list
            |-- RuleRegistry::with_all_rules()
            |     `-- .enabled(&config)     filter out ignored rules
            |-- resolve_pattern()           bare dir -> glob; else parse glob
            |-- discover_files()            walk filesystem, match glob
            |
            `-- for each file:
                  analyze_file(path, source, rules)
                    |-- FileContext::parse()      ONE parse, shared tree + line index
                    |-- Suppressions::parse()     scan comments for ignore markers
                    `-- for each rule:
                          if !rule_supports(rule, path) { skip }
                          rule.check(&context) -> Vec<Violation>
                          drop violations the suppressions cover
                  |
                  `-- format_reports()       ESLint-style output to stdout
```

## Why `FileContext` instead of `(&str, &Path)`

The original specification called for:

```rust
fn check(&self, source: &str, path: &Path) -> Vec<Violation>;
```

The implemented signature is:

```rust
fn check(&self, file: &FileContext) -> Vec<Violation>;
```

**The problem with `&str`.** A raw-source signature gives a rule nothing it can
use directly — every rule's first act would have to be parsing the file itself.
With 3 rules that is 3 full AST builds of every file, and the cost grows
linearly with each rule added. Parsing dominates this tool's runtime; the
detection logic is a single `descendants()` walk per rule, which is cheap by
comparison.

`FileContext` inverts that: the runner parses once, and every rule borrows the
same tree and the same line index.

| Field | Purpose |
| --- | --- |
| `tree: JsSyntaxNode` | The parsed AST, shared by every rule |
| `line_starts: Vec<usize>` | Byte-offset index, built once, powers `line_col()` |
| `source: &str` | Original text, still reachable |
| `path: &Path` | File path, still reachable |
| `parsed_cleanly: bool` | Whether the parse produced errors |

### What it costs

Rules do **not** own suppression or extension filtering. A rule cannot decide
"this violation is ignored" or "I don't handle `.ts`" — the runner does both, so
`Rule` is deliberately not self-contained. Reading a rule in isolation does not
tell you the whole story of how its violations reach the output; you have to
know `analyze_file` exists. That is a real readability cost, accepted knowingly.

### What it buys

**Measured**, running against the dashboard's `src/` (4393 files, warm cache):

| Rules enabled | Wall time |
| --- | --- |
| 1 (`no-native-map` only) | 1.96s |
| 3 (all) | 2.40s |

Two rules cost 0.44s to add, so a rule's tree walk is ~0.22s while the fixed
read-and-parse cost is ~1.74s — **roughly 72% of the run is parsing.** Under the
`&str` design that fixed cost would be paid once per rule:

| Rules | `FileContext` (1 parse) | `&str` (N parses) | Speedup |
| --- | --- | --- | --- |
| 3 (today) | ~2.4s | ~5.5s | ~2.3x |
| 10 (hypothetical) | ~3.9s | ~18s | ~4.6x |

So the win is ~2.3x today and widens as rules are added. It is **not** the
3x–10x that a naive "3 rules means 3x the parsing" estimate suggests, because
each rule also contributes its own walk cost, which no design can share away.

The second benefit is organisational: **new rules inherit suppression and
extension filtering for free.** A rule author writes detection logic and
nothing else — there is no ignore-comment parsing to get subtly wrong, and no
way for two rules to disagree about what `// biome-ignore-line` means. See
"suppression and extension filtering live in the runner" below.

**Nothing is lost.** The spec's two parameters are still available as
`file.source()` and `file.path()`, so a rule that genuinely needs raw text or
the filename can have it. The line index is a smaller instance of the same win:
computing line/column from a byte offset needs a sorted table of line starts,
and building that per-rule would be the same duplicated work as parsing.

### Verdict

**This is the right design for a multi-rule linter,** and it should stay. The
whole point of the architecture is that rule count is expected to grow — the
registry exists to make adding rules easy — and this is precisely the axis along
which the `&str` signature degrades. Trading a little rule-level
self-containment for a shared parse and uniform suppression is the correct call.

**Reverting is mechanical** if strict parity with the spec ever matters more:
change the trait signature and have each rule open with
`let file = FileContext::parse(source, path);`. The runner's suppression and
extension filtering are unaffected. Do not do this to satisfy the letter of a
spec — paying N parses to match a signature buys nothing.

## Decision: pinning all six Biome crates to `=0.5.7`

`Cargo.toml` lists six Biome crates, but the code only imports three:

```toml
biome_js_parser  = "=0.5.7"   # used
biome_js_syntax  = "=0.5.7"   # used
biome_rowan      = "=0.5.7"   # used
biome_parser     = "=0.5.7"   # NOT used — pin only
biome_diagnostics = "=0.5.7"  # NOT used — pin only
biome_console    = "=0.5.7"   # NOT used — pin only
```

**The problem.** `biome_rowan 0.5.8` is a breaking change despite the patch-level
version bump — it altered `SyntaxKind::is_trivia` and added a `Debug` bound to
`SendNode`. `biome_js_syntax 0.5.7` does not compile against it.

Cargo's semver unification treats `0.5.7` and `0.5.8` as compatible and picks
the highest. So even with `biome_rowan` pinned, cargo would resolve
`biome_parser` (a transitive dependency of `biome_js_parser`) to `0.5.8`, which
in turn demands `biome_rowan 0.5.8` features, and the build fails with errors
about `is_trivia` and missing `Debug` impls.

**The fix.** Naming `biome_parser`, `biome_diagnostics` and `biome_console` as
direct dependencies with `=` pins forces cargo to hold the entire graph at
`0.5.7`. They are declared purely to constrain resolution — there is no `use`
statement for any of them anywhere in `src/`.

**This is load-bearing.** Do not "clean up" these three unused dependencies, and
do not relax `=0.5.7` to `0.5` or `^0.5.7`, without rebuilding from a clean
`target/` and a deleted `Cargo.lock`. `Cargo.lock` is committed for the same
reason. If you do want to move to a newer Biome, move all six together and
expect to fix compile errors in the rules.

## Decision: suppression and extension filtering live in the runner

Both concerns are handled in `analyzer/runner.rs::analyze_file`, never inside a
rule:

```rust
for rule in rules {
    if !rule_supports(*rule, path) { continue; }        // extension filter
    for violation in rule.check(&context) {
        if suppressions.is_suppressed(violation.line, violation.rule) {
            continue;                                   // suppression filter
        }
        violations.push(violation);
    }
}
```

**Why centralize.** Three reasons:

1. **Consistency by construction.** If each rule parsed its own ignore comments,
   a new rule could easily get the syntax subtly wrong — supporting
   `biome-ignore-line` but not the `--` justification suffix, say — and users
   would hit inconsistent behaviour across rules. There is exactly one
   implementation, in `suppress/mod.rs`, so all rules behave identically.
2. **Rules stay minimal.** A new rule author writes detection logic and nothing
   else. No boilerplate to forget.
3. **One comment scan per file.** `Suppressions::parse` runs once per file, not
   once per rule, for the same reason the parse does.

The suppression key is `(line, rule_name)`. A violation is dropped when the line
it is **reported on** carries that rule's name. Note the consequence for
`reselect-arity-match`: it reports at the result function's position, which may
not be the `createSelector` line, so the ignore comment goes on the result
function's line.

Extension filtering reads `rule.supported_extensions()` and compares against the
path's extension, tolerating both `".js"` and `"js"` forms. Today all three
rules declare `JS_EXTENSIONS` (`[".js", ".jsx"]`), but the mechanism is there so
a future TypeScript-only or JSX-only rule needs no special casing.

## Decision: `ignoreBiomeExtensionRules` in `package.json`

Rules are disabled by name from the nearest `package.json` at or above the
working directory:

```json
{
  "ignoreBiomeExtensionRules": ["no-native-map"]
}
```

**Why `package.json` rather than a dedicated config file.** The tool is meant to
sit alongside Biome in a JS project that already has a `package.json`. Adding a
`.custombiomelintrc` would be one more file for developers to know about, and
the config is a single array of strings — it does not justify its own file.

**Why the key is named that.** It is namespaced enough not to collide with
anything npm or Biome uses, and it reads as "rules of the Biome-extension tool
that should be ignored."

**Behaviour.** `PackageConfig::load` walks up from the working directory via
`Path::ancestors()` and takes the first `package.json` it finds. A missing file
is **not an error** — the tool simply runs with all rules enabled, which keeps
it usable outside a JS project (e.g. running it against `fixtures/`). Malformed
config is reported as a warning on stderr rather than a hard failure, and the
run continues:

- unreadable file -> warning, all rules enabled
- invalid JSON -> warning, all rules enabled
- key present but not an array -> warning, key ignored
- array containing a non-string -> warning naming the bad entry, other entries honoured

The registry then exposes `.enabled(&config)` and `.ignored(&config)`; the CLI
uses the first to drive the run and the second to report what was skipped under
`-v`.

## Module-by-module walkthrough

### `src/lib.rs` (38 lines)

Library root. Declares the six modules, re-exports the public surface
(`Rule`, `RuleRegistry`, `FileContext`, `Violation`, `PackageConfig`,
`Suppressions`, …), and provides the one convenience function for embedding:

```rust
pub fn lint_source(source: &str, path: &Path, rules: &[&dyn Rule]) -> Vec<Violation>
```

The doc comment on this function is the crate's single doctest.

### `src/bin/custom-biome-lint.rs` (7 lines)

The binary. Calls `cli::run()` with `std::env::args().skip(1)` and the current
directory, and returns its `ExitCode`. Deliberately trivial so that everything
is testable through the library.

### `src/cli/`

| File | Role |
| --- | --- |
| `args.rs` | `CliArgs::parse` — hand-rolled arg parsing, no `clap` dependency |
| `output.rs` | `Reporter` (verbosity-gated stderr logging), `HELP` text, `vlog!`/`dlog!` macros |
| `mod.rs` | `run()` — orchestrates config, registry, discovery, analysis, reporting |

`args.rs` supports clustered and repeated short flags (`-v`, `-vv`, `-vvv`,
`-vd`), saturating verbosity at 3, plus `--debug` (which outranks `-vvv` as
level 4), `--trace`, `--help`, `--version`, and one positional pattern. A second
positional is an error rather than being silently ignored.

`output.rs` keeps a strict stream split: **diagnostics go to stdout, all logging
and warnings go to stderr.** That is what makes `custom-biome-lint > report.txt`
produce a clean report. The `vlog!` macro captures `file!()`/`line!()` at the
call site so `--trace` can prefix log lines with their source location — this
only works as a macro, which is why `Reporter::emit` takes a `location` string
rather than computing it.

`mod.rs` is the orchestration layer and the only place that touches the
filesystem for reading source files. It also handles the "pattern root does not
exist" case, which is the exit-code-2 path.

### `src/config/`

`package_config.rs` — `PackageConfig::load`, upward `package.json` discovery,
`ignoreBiomeExtensionRules` parsing, and the warning collection described above.
`serde_json` is used with `Value` rather than a derived struct, because the tool
must tolerate arbitrary unrelated keys in a real `package.json` and must not
fail on fields it does not understand.

### `src/analyzer/`

| File | Role |
| --- | --- |
| `file_matcher.rs` (324 lines) | `GlobSet` — hand-rolled glob matching |
| `mod.rs` | `discover_files`, `resolve_pattern`, directory-skip list |
| `runner.rs` | `FileContext`, `analyze_file`, `rule_supports` |

`file_matcher.rs` implements `*`, `?`, `**`, and `{a,b}` brace sets from
scratch. **Why not the `glob` crate:** it keeps the dependency set to Biome's
parser crates plus `serde_json`, which matters for the portability goal (see
below), and brace expansion — which the default pattern
`src/**/*.{js,jsx}` needs — is not something `glob` provides anyway. Brace
expansion happens up front in `expand_braces`, turning one pattern into a set of
brace-free patterns; `is_match` then tests each. `root_dir()` extracts the
literal prefix before the first magic character so the filesystem walk can start
there instead of at the repository root.

**Absolute patterns.** `root_dir()` preserves a leading `/`, and `match_key()`
compares absolute patterns against the absolute path rather than one made
relative to the walk root. Both are needed for `custom-biome-lint /abs/path` to
work: splitting `/a/b` yields a leading empty segment, and collecting it away
would silently turn the walk root relative so it got joined onto the cwd
(producing `<cwd>/a/b`), while stripping the walk root from the match key would
leave a relative key that no absolute alternative can ever match. The second
failure mode is the dangerous one — it reports a clean run instead of an error.

`mod.rs::discover_files` walks from `pattern.root_dir()`, skipping
`node_modules`, `target`, `dist`, `build`, `coverage`, `vendor`, and any
dot-directory. It returns a `Discovery` carrying not just the file list but
`dirs_scanned`, `dirs_skipped` and `files_considered`, which is what the `-vv`
output reports.

`mod.rs::resolve_pattern` implements the bare-directory shorthand: if the input
has no glob metacharacters and names an existing directory, it becomes
`<dir>/**/*.{js,jsx}`. This exists because `custom-biome-lint src` is the
obvious thing to type, and without it the pattern would match nothing and the
tool would report a clean run — a silent false negative, the worst possible
failure mode for a linter. Four regression tests cover this after a bug where a
missing brace escape produced `fixtures/**/*.js,jsx` instead of
`fixtures/**/*.{js,jsx}`.

`runner.rs` holds `FileContext` (discussed above) and `analyze_file`. Violations
are sorted by `(line, col, rule)` before being returned, so output ordering is
deterministic regardless of the order rules are registered in. The parser is
always invoked with `JsFileSource::jsx()`, because this codebase has JSX inside
plain `.js` files and JSX is a superset of the syntax that plain JS files use.

### `src/rules/`

| File | Role |
| --- | --- |
| `rule.rs` (27 lines) | The `Rule` trait |
| `registry.rs` | `RuleRegistry` — the list of all rules, plus config filtering |
| `no_native_map.rs` (298 lines) | Immutable.js `Map` rule |
| `no_arrow_function_create_selector.rs` (120 lines) | Memoization rule |
| `reselect_arity_match.rs` (115 lines) | Arity rule |
| `mod.rs` | Re-exports, `JS_EXTENSIONS`, `JS_PATTERN` constants |

The trait is `Send + Sync` so a future parallel implementation over files needs
no trait change. `registry.rs::with_all_rules` is the single registration point
— the one place to edit when adding a rule. `supported_extensions()` on the
registry is the union across rules, and `default_pattern()` derives
`src/**/*.{js,jsx}` from that union rather than hardcoding it, so registering a
`.ts` rule automatically widens the default glob.

`no_native_map.rs` is by far the largest rule because it is the only stateful
one: it must track how `immutable` entered the file (default import, named
import, `require`, destructuring, namespace alias) before it can decide whether
a bare `Map` is native. It relies on `descendants()` being preorder, so a
declaration is seen before the identifiers inside it — the same order the
original ESLint rule's visitors fire in. See [RULES.md](RULES.md) for the
detection details and the known false-positive class.

### `src/suppress/`

`mod.rs` — `Suppressions::parse` scans each line for `biome-ignore-line` (applies
to that line) and `biome-ignore-next-line` (applies to the following line),
building a map from line number to the rules suppressed there. A bare marker sets
an `all` flag instead of listing names.

`find_suppression_comments` exposes the same scan as structured
`SuppressionComment` values — comment line, target line, rules, and an
`append_at` offset just past the last rule name. The fixer uses this to splice
extra rule names into a comment that already exists, ahead of any `--`
justification.

Details worth knowing:

- **A marker only counts inside a real comment.** `comment_body` finds the first
  `//` or `/*` and only looks after it, so `const x = 'biome-ignore-line
  no-native-map'` is not a suppression. This is a deliberate lexical
  approximation rather than a token-level check — good enough given the marker
  text would be bizarre inside a string, and it avoids a second tree walk.
- **A bare marker suppresses every rule.** `// biome-ignore-line` with no rule
  name sets `LineSuppression::all`. The cost is the reason to avoid writing one:
  a blanket-ignore left behind after a refactor silently hides violations from
  rules that did not exist when it was written. `--write-fix` therefore always
  emits explicit rule names, and never a bare marker.
- **Only the first marker on a line is parsed.** `find_suppression_comments`
  takes one marker per line, checking `-next-line` before `-line`. A second
  marker appended to the same line would be swallowed, which is why the fixer
  refuses trailing placement on a line that already carries one.

A `--` token ends the rule list, so `// biome-ignore-line no-native-map -- keys
are DOM nodes` works.

### `src/fixer.rs`

Backs `--write-fix`. `plan_file` is pure — source in, rewritten source plus a
list of `FileChange`s out — so every placement rule is unit-testable without
touching disk. `Fixer::apply_suppressions` is the thin IO wrapper around it.

The whole module is organised around one question: *where is it provably safe to
put a comment?* Three checks answer it.

- **A line lexer** (`line_states`) tracks the state at the start and end of every
  line, so a line inside a multi-line template literal or block comment is never
  written to. It deliberately does not understand regex literals or `${}`
  nesting; both blind spots leave it believing it is inside a string, which can
  only make the fixer decline a placement, never accept an unsafe one. A quote
  state that reaches end-of-line without a backslash continuation is reset to
  code, so a regex like `/['"]/` cannot swallow the rest of the file.
- **JSX child detection** (`JsxText`) collects `JSX_CHILD_LIST` and
  `JSX_EXPRESSION_CHILD` ranges from the tree. In JSX children a `//` comment is
  *rendered text*, so the `{/* ... */}` form is used there — and only on its own
  line, because `{expr} {/* ... */}` leaves a whitespace-only text node that
  React renders as a space. Own-line placement is inert, since JSX discards a
  whitespace run containing a newline.
- **Post-hoc verification** (`verify`) re-parses the rewritten source and asserts
  that every rule the plan claimed to suppress really is suppressed at the code's
  new line number. A file that fails verification is left untouched. This is
  cheap insurance against a placement bug producing a file whose suppressions
  silently do not apply.

Two consequences of how `suppress` works constrain placement:

- Only the first marker on a line is parsed, so a trailing comment is never added
  to a line that already carries a marker — it would be swallowed. Such lines get
  own-line placement instead.
- When a comment already targets the offending line, its rule list is extended
  rather than a second comment added, which is what makes repeated runs
  idempotent.

A file with parse errors is never rewritten: the JSX detection would be
unreliable and the risk of corrupting it is real. Anything unplaceable is
surfaced as an `Unfixable` warning and makes the run exit non-zero, so a violation
never disappears silently.

### `src/diagnostics/`

| File | Role |
| --- | --- |
| `violation.rs` | `Violation { line, col, rule, message, severity }`, `Severity` |
| `formatter.rs` | `FileReport`, `Totals`, `format_reports`, `tally` |

`format_reports` produces ESLint's format — a path header, then `line:col
severity message rule` rows with the `line:col` column right-padded to align
within a file, then a one-line summary. The column widths are computed per file,
matching ESLint's behaviour.

The summary reports file count and wall time on both paths:

```
✖ 7 errors in 3 files (9 files checked in 5ms)
✔ No violations found (4393 files checked in 2.15s)
```

`Totals::elapsed` carries the duration. `tally()` leaves it zero — it counts
violations and has no business reading a clock — and `run()` fills it in from an
`Instant` taken as its first statement, so the figure covers config loading and
file discovery, not just analysis. Sub-second runs are printed in whole
milliseconds, because `0.01s` reads worse than `5ms`.

## Portability

The package is self-contained and can be lifted into its own repository,
published to npm, or vendored as a git submodule with no edits:

- Dependencies are Biome's parser crates plus `serde_json`. Glob matching is
  in-tree specifically to avoid another dependency.
- Nothing reads from the surrounding repository. The only external input is the
  `package.json` discovered at runtime, and its absence is handled.
- `Cargo.lock` is committed, so a fresh clone reproduces the exact dependency
  graph that the version pins are protecting.

This was verified by rsync-copying the directory (minus `target/`) outside the
repository and building from scratch; see [TESTING.md](TESTING.md).
