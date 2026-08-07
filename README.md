# custom-biome-lint

Standalone linter for the Reselect / Redux / Immutable patterns that Biome does
not cover. It exists so the three remaining custom ESLint rules in this
codebase can be retired without losing their coverage during the Biome
migration.

Written in Rust on Biome's own JS parser, so it sees the same AST Biome does and
handles JSX inside `.js` files.

## Rules

| Rule | What it catches |
| --- | --- |
| `no-native-map` | `new Map()` where Immutable.js `Map` is expected. Understands `import { Map } from 'immutable'`, `import Immutable from 'immutable'`, `require('immutable')`, `const { Map } = Immutable` and `Immutable.Map` aliases. |
| `no-arrow-function-create-selector` | `createSelector` wrapped in an arrow function, which rebuilds the selector on every call and defeats memoization. Names matching `/^make[A-Z]/` are treated as deliberate factories and allowed. |
| `reselect-arity-match` | A `createSelector` result function whose parameter count does not match the number of input selectors. |

Each rule is a direct port of the corresponding rule in `eslint-rules/`, and the
ports are deliberately behaviour-for-behaviour rather than "improved", so that
enabling this tool produces exactly the findings ESLint produced. Full details,
including one known false-positive class, are in [docs/RULES.md](docs/RULES.md).

## Documentation

| Document | Contents |
| --- | --- |
| [docs/SETUP.md](docs/SETUP.md) | Installing Rust from zero, building, running the binary |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Module walkthrough and why each non-obvious decision was made |
| [docs/RULES.md](docs/RULES.md) | Each rule with before/after examples, known quirks, real-codebase findings |
| [docs/ADDING_A_RULE.md](docs/ADDING_A_RULE.md) | Step-by-step guide with a worked example |
| [docs/TESTING.md](docs/TESTING.md) | Test suites, clippy, fixture and real-tree runs, portability check |
| [docs/CI_CD_INTEGRATION.md](docs/CI_CD_INTEGRATION.md) | Husky and GitLab CI wiring (not yet applied) |
| [docs/MIGRATION_NOTES.md](docs/MIGRATION_NOTES.md) | The 8 suppression comments still to translate |

## Build

```sh
cargo build --release        # or: npm run build
```

The binary lands at `target/release/custom-biome-lint`.

## Usage

```
custom-biome-lint [PATTERN] [FLAGS]
```

`PATTERN` is a glob, defaulting to `src/**/*.{js,jsx}`. `*`, `?`, `**` and
`{a,b}` brace sets are supported. A bare directory is expanded for you, so
`custom-biome-lint src` means `src/**/*.{js,jsx}`.

```sh
custom-biome-lint                          # lint src/**/*.{js,jsx}
custom-biome-lint 'src/store/**/*.js'      # narrow the scope
custom-biome-lint src/reducers             # bare directory shorthand
custom-biome-lint -v                       # show config, rules and pattern
```

Quote globs so your shell does not expand them first.

### Flags

| Flag | Effect |
| --- | --- |
| `--write-fix` | Add a suppression comment for every violation, in place |
| `--dry-run` | With `--write-fix`, report the comments without writing |
| `--format <text\|json>` | Diagnostics output format (default: `text`). Not supported with `--write-fix`. |
| `-v`, `--verbose` | Config source, enabled/skipped rules, resolved pattern |
| `-vv` | Brace expansion, walk root, discovery counts |
| `-vvv` | Per-file: rules run, violation count, line count |
| `-d`, `--debug` | Internal state and every step (outranks `-vvv`) |
| `--trace` | Prefix each log line with its source location |
| `-h`, `--help` | Usage |
| `-V`, `--version` | Version |

Diagnostics go to **stdout**; all logging and warnings go to **stderr**, so
`custom-biome-lint > report.txt` captures just the report.

### Exit codes

| Code | Meaning |
| --- | --- |
| `0` | No violations (with `--write-fix`: every violation was suppressed) |
| `1` | Violations found, or a violation could not be suppressed |
| `2` | Bad usage, or the pattern's root directory does not exist |

A `--write-fix --dry-run` run that has anything to write also exits `1`, so it
works as a CI check.

## Output

ESLint's format — a path header, aligned `line:col  severity  message  rule`
rows, then a summary:

```
src/components/MapBuilder/MapCanvas.jsx
  120:30  error  Use Immutable.js Map instead of native Map.  no-native-map

src/selectors/users.js
  12:64  error  createSelector expects 1 parameter(s) in the result function, but found 2.  reselect-arity-match

✖ 2 errors in 2 files
```

### JSON output

`--format json` prints a single stable JSON document to stdout instead — no
other stdout content in that mode, so a consumer can parse it directly:

```sh
custom-biome-lint --format json > report.json
```

```json
{
  "version": 1,
  "files": [
    {
      "path": "src/selectors/users.js",
      "violations": [
        {
          "line": 12,
          "col": 64,
          "severity": "error",
          "rule": "reselect-arity-match",
          "message": "createSelector expects 1 parameter(s) in the result function, but found 2."
        }
      ]
    }
  ],
  "summary": {
    "errors": 1,
    "warnings": 0,
    "filesWithViolations": 1,
    "filesChecked": 9,
    "elapsedMs": 7,
    "clean": false
  }
}
```

The schema is additive-only across versions: existing fields never change
meaning or disappear, so a consumer that reads only the fields it knows about
keeps working after an upgrade.

## Configuration

Rule severities are set by name in the nearest `package.json` at or above the
working directory, via `ignoreBiomeExtensionRules`. Two shapes are accepted:

```json
{
  "ignoreBiomeExtensionRules": ["no-native-map"]
}
```

The array form is shorthand for turning listed rules fully `"off"`. For finer
control, use the object form with `"off"` / `"warn"` / `"error"` per rule:

```json
{
  "ignoreBiomeExtensionRules": {
    "no-native-map": "off",
    "reselect-arity-match": "warn"
  }
}
```

`"off"` disables the rule entirely, same as the array form. `"warn"` and
`"error"` don't change whether the rule runs — only the severity of what it
reports. `"warn"` violations are still printed and still counted, but (unlike
`"error"`, the default) they don't make the run exit non-zero, so a rule you
want visibility into without blocking CI can be turned down without silencing
it. A rule with no entry keeps its default severity (`"error"`).

A missing `package.json` is not an error — every rule stays enabled at its
default severity.

## Suppressions

Two comment forms:

```js
const cache = new Map(); // biome-ignore-line no-native-map

// biome-ignore-next-line no-native-map, reselect-arity-match
const other = new Map();
```

Comma- and space-separated names both work. Text after a `--` token is ignored,
so a justification can be written inline:

```js
const nodeCache = new Map(); // biome-ignore-line no-native-map -- keys are DOM nodes
```

A marker with **no** rule names suppresses every rule on its target line:

```js
const anything = new Map(); // biome-ignore-line
```

Prefer naming the rule. A bare marker also hides rules added later, and rules
that start firing on that line for unrelated reasons, so it trades away the
warning you would otherwise get.

Inside JSX children a `//` comment is rendered text, not a comment, so the brace
form is required there — and is what `--write-fix` emits:

```jsx
<div>
  {/* biome-ignore-next-line no-native-map */}
  {new Map().get(key)}
</div>
```

A marker only counts inside a real comment; the same text in a string literal is
not a suppression. Only the first marker on a line is parsed, so a line cannot
carry two markers. Suppressions apply to the line the violation is *reported*
on, which for `reselect-arity-match` is the line of the result function, not
necessarily the `createSelector` call.

## Adding suppressions automatically

`--write-fix` adds a suppression comment for every violation it finds, which is
how an existing codebase is brought to a clean baseline:

```bash
custom-biome-lint --write-fix --dry-run src   # report what would change
custom-biome-lint --write-fix src             # apply it
```

Placement rules:

- a trailing `biome-ignore-line` when the resulting line stays within 100
  columns, otherwise `biome-ignore-next-line` on its own line above, indented to
  match;
- the `{/* ... */}` form, always on its own line, when the insertion point is in
  JSX children — a trailing brace comment there would leave a whitespace-only
  text node that React renders as a space;
- several violations on one line share a single comment;
- an existing suppression comment is extended with the missing rule names rather
  than duplicated, so re-running is idempotent.

Anything that cannot be suppressed without risking a change in meaning is left
alone and reported as a warning: a line inside a multi-line template literal or
block comment, and any file with parse errors. Every rewrite is re-parsed and
re-checked before it is written, and `--write-fix` exits non-zero if any
violation was left unsuppressed.

## ESLint parity

These rules reproduce their ESLint counterparts' findings exactly. One
consequence is worth knowing up front: **`no-native-map` flags any identifier
named `Map`, including member names**, so `new mapboxgl.Map({...})` is reported.
That is faithful to the original rule, not a port bug, and it is why eight call
sites in this codebase already carry disable comments.

Those eight comments need translating to this tool's syntax — the tool
deliberately does **not** honour `eslint-disable*` comments. See
[docs/MIGRATION_NOTES.md](docs/MIGRATION_NOTES.md) for the exact diff and the
reasoning, and [docs/RULES.md](docs/RULES.md) for each rule's limitations.

## Adding a rule

1. Create `src/rules/my_rule.rs` and implement `Rule`.
2. Register it in `RuleRegistry::with_all_rules` in `src/rules/registry.rs`.
3. Add `fixtures/my_rule/{valid,invalid,suppressed}.js`.
4. Add a `mod my_rule` block to `tests/integration.rs`.

Suppression and extension filtering are handled by the runner, so a rule
contains detection logic and nothing else. Full guide with a worked example:
[docs/ADDING_A_RULE.md](docs/ADDING_A_RULE.md).

## Library use

```rust
use std::path::Path;
use custom_biome_lint::{lint_source, RuleRegistry};

let registry = RuleRegistry::with_all_rules();
let violations = lint_source(source, Path::new("a.js"), &registry.all());
```

## Layout

```
src/
  bin/custom-biome-lint.rs   CLI entry point
  lib.rs                     library exports
  cli/                       arg parsing, help, verbosity-gated logging
  config/                    package.json ignore list
  analyzer/                  file discovery, glob matching, single-pass runner
  rules/                     Rule trait, registry, one module per rule
  suppress/                  biome-ignore-line / -next-line parsing
  fixer.rs                   --write-fix: safe suppression-comment placement
  diagnostics/               Violation type and ESLint-style formatter
fixtures/<rule_name>/        valid.js, invalid.js, suppressed.js, edge-cases.js per rule
tests/integration.rs         end-to-end rule, config and pattern tests
docs/                        architecture, rules, testing, setup, CI, migration
.github/workflows/ci.yml                  build, test, fmt, clippy, audit, deny
.github/workflows/biome-upgrade-check.yml monthly + on-demand: can we bump Biome yet?
rustfmt.toml                  formatting config (cargo fmt)
deny.toml                     license/advisory/source policy (cargo deny)
```

## Portability

Self-contained: the only dependencies are Biome's parser crates and
`serde_json`. Glob matching is implemented in `analyzer/file_matcher.rs` rather
than pulled from a crate, and nothing reads from the surrounding repository
except the `package.json` it discovers at runtime. The directory can be moved to
its own repository, published to npm, or used as a git submodule without edits.

The Biome crates are pinned to exactly `0.5.7`, and `biome_parser`,
`biome_diagnostics` and `biome_console` are declared as direct dependencies
without being imported purely to hold the graph at that version. **Do not remove
them or loosen the pins** — see
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#decision-pinning-all-six-biome-crates-to-057).

## Testing

```sh
cargo test                                    # 92 tests: 64 unit + 27 integration + 1 doctest
cargo fmt --all -- --check                    # no diff expected
cargo clippy --all-targets -- -D warnings     # no warnings expected
cargo audit                                   # no advisories beyond .cargo/audit.toml's ignore list
cargo deny check                              # licenses, bans, sources all ok
./target/release/custom-biome-lint fixtures   # 12 errors across 6 files
```

Full procedure, including running against the real dashboard tree and how the
portability check was done: [docs/TESTING.md](docs/TESTING.md).
