# package.json setup and dependency management

Field-by-field reference for both sides: the **consumer** (the <PRIVATE_REPO>, or any
project running the linter) and the **tool** (custom-biome-lint's own manifest).

For the surrounding workflow see [INTEGRATION_GUIDE.md](INTEGRATION_GUIDE.md); for
CI specifics see [CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md).

---

## A. Consumer package.json

### Current state of the <PRIVATE_REPO>

Worth being precise, because the target state below is not what is in the repo
today:

```jsonc
{
  "scripts": {
    "eslint": "npx eslint \"src/**/*.{js,jsx}\" --cache --cache-location ./node_modules/.cache/eslint/.eslintcache",
    "eslint:fix": "npx eslint \"src/**/*.{js,jsx}\" --fix",
    "lint": "eslint ./src",
    "prettier:check": "prettier --check \"src/**/*.{js,jsx}\"",
    "prettier:fix": "prettier --write \"src/**/*.{js,jsx}\""
  },
  "devDependencies": {
    "eslint": "^9.38.0"
    // NOTE: @biomejs/biome is NOT installed yet.
  }
}
```

ESLint and Prettier are still the enforced path — `.husky/pre-push` runs
`yarn eslint && yarn prettier:check`, and the `lint` CI job runs the same pair.
Biome is not yet a dependency. Any example that composes `biome check` with
`lint:custom` describes the **target** state after the Biome migration, not
something you can run today. See [MIGRATION_NOTES.md](MIGRATION_NOTES.md) for
where that migration actually stands.

### Adding the custom linter now (alongside ESLint)

This works against the repo as it exists:

```json
{
  "scripts": {
    "lint:custom": "test -x custom-biome-lint/target/release/custom-biome-lint || yarn lint:custom:build; custom-biome-lint/target/release/custom-biome-lint src",
    "lint:custom:build": "cargo build --release --manifest-path custom-biome-lint/Cargo.toml",
    "lint:custom:fix": "custom-biome-lint/target/release/custom-biome-lint src --write-fix",
    "lint:custom:dry": "custom-biome-lint/target/release/custom-biome-lint src --write-fix --dry-run"
  }
}
```

Paths assume the in-repo layout (`custom-biome-lint/`). For an external tool at
`tools/custom-biome-lint/`, adjust accordingly — see
[INTEGRATION_GUIDE.md](INTEGRATION_GUIDE.md).

### Target state, after the Biome migration

```json
{
  "scripts": {
    "lint": "biome check src && yarn lint:custom",
    "lint:fix": "biome check --write src && yarn lint:custom:fix",
    "lint:custom": "custom-biome-lint/target/release/custom-biome-lint src",
    "lint:custom:build": "cargo build --release --manifest-path custom-biome-lint/Cargo.toml",
    "lint:custom:fix": "custom-biome-lint/target/release/custom-biome-lint src --write-fix",
    "lint:custom:dry": "custom-biome-lint/target/release/custom-biome-lint src --write-fix --dry-run"
  },
  "devDependencies": {
    "@biomejs/biome": "^2.5.5"
  }
}
```

`&&` short-circuits: if `biome check` fails, `lint:custom` never runs and its
findings stay hidden until the Biome errors are fixed. Use `;` instead if you want
both reports in one pass, accepting that the exit code then reflects only the last
command.

Do **not** chain the linter into `build:prod` or `build:stage`. A failing lint
should not be able to block a deploy that is otherwise sound; keep enforcement in
CI and the pre-push hook.

### `ignoreBiomeExtensionRules`

The tool's only configuration, read from the nearest `package.json` at or above the
linted files. Two shapes are accepted:

```json
{
  "ignoreBiomeExtensionRules": ["no-native-map"]
}
```

```json
{
  "ignoreBiomeExtensionRules": {
    "no-native-map": "off",
    "reselect-arity-match": "warn"
  }
}
```

| Behaviour | Detail |
| --- | --- |
| Location | Nearest `package.json` at or above the linted path |
| Type | Array of strings (shorthand for `"off"`), or an object mapping rule name to `"off"`/`"warn"`/`"error"`; anything else emits a warning and is ignored |
| Values | Rule names exactly as in [RULES.md](RULES.md), kebab-case |
| `"off"` | Rule does not run at all |
| `"warn"` | Rule runs; violations are reported but do not fail the run |
| `"error"` | Default. Rule runs; violations fail the run |
| Missing file/key | Not an error — all rules run at `"error"` |
| Unknown rule name | Silently accepted (no error), so typos disable nothing |

That last row is the trap: `"no-native-maps"` looks like it works and does nothing.
Verify with `-vv`, which prints the config file found and which rules are enabled
versus skipped:

```sh
yarn lint:custom -- -vv
```

Use this to disable a rule wholesale. For individual findings, prefer a
suppression comment with a justification:

```js
const cache = new Map(); // custom-biome-ignore-line no-native-map -- perf-critical hot path
```

The `--` justification is not parsed, but it is the only record of *why* the
exception exists. Suppression syntax in full: [README.md](../README.md#suppressions).

### Not a devDependency

With the submodule or local-copy setup, the linter is **not** an npm dependency —
it is a directory in the repo built by cargo. Adding a `devDependencies` entry
pointing at a git URL only works if you have published an npm-consumable package
([PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md)); a git dependency on a Rust repo installs
source that npm cannot build.

Only Option 3 uses a dependency entry:

```json
{
  "devDependencies": {
    "custom-biome-lint": "^0.1.0"
  },
  "scripts": {
    "lint:custom": "custom-biome-lint src"
  }
}
```

Note the script has no path — npm puts the binary on `PATH` via
`node_modules/.bin`.

---

## B. The tool's own package.json

Current contents:

```json
{
  "name": "custom-biome-lint",
  "version": "0.1.0",
  "description": "Standalone linting tool for Reselect/Redux patterns not in Biome",
  "main": "target/release/custom-biome-lint",
  "bin": { "custom-biome-lint": "target/release/custom-biome-lint" },
  "scripts": {
    "build": "cargo build --release",
    "test": "cargo test",
    "lint": "cargo clippy"
  },
  "files": ["src", "docs", "Cargo.toml", "Cargo.lock", "README.md"],
  "keywords": ["lint", "biome", "reselect", "redux", "immutable"],
  "license": "MIT"
}
```

This manifest exists so the directory can be published or consumed by npm. Nothing
in the Rust build reads it — `cargo` uses `Cargo.toml`.

Three issues to fix before a first publish:

**1. `files` omits the binary that `main`/`bin` point at.** npm force-includes
`main` and `bin` targets regardless of `files` and `.gitignore`, so the binary
ships anyway — verified with `npm pack --dry-run`. Relying on that is implicit;
list it:

```json
"files": [
  "target/release/custom-biome-lint",
  "src", "docs", "fixtures",
  "Cargo.toml", "Cargo.lock", "README.md"
]
```

**2. No platform guard.** The binary is architecture-specific
(`Mach-O 64-bit executable arm64` when built on an Apple Silicon Mac). Add:

```json
"os": ["darwin"],
"cpu": ["arm64"]
```

so npm refuses the install instead of failing at lint time. See
[PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md#the-platform-problem-read-this-first).

**3. `lint` script is weaker than the CI gate.** `cargo clippy` skips tests and
fixtures; CI runs `cargo clippy --all-targets`. Match them:

```json
"lint": "cargo clippy --all-targets"
```

Also worth adding: a `repository` field (see
[EXTRACT_TO_SEPARATE_REPO.md](EXTRACT_TO_SEPARATE_REPO.md#4-fix-the-repository-metadata),
which also notes that `Cargo.toml` currently points at a non-existent GitHub URL).

### Version must match Cargo.toml

`Cargo.toml` drives `--version`; `package.json` drives the registry. Nothing keeps
them in sync locally, so bump both in the same commit. The CI job in
[EXTRACT_TO_SEPARATE_REPO.md](EXTRACT_TO_SEPARATE_REPO.md#5-add-ci) asserts they
match, failing the pipeline rather than shipping a mismatch.

---

## C. Dependencies

### Install

| What | Scope | How |
| --- | --- | --- |
| Rust toolchain | Global, one-time per machine | `rustup` — see [SETUP.md](SETUP.md) |
| Cargo crates | Per project, automatic | `cargo build` reads `Cargo.lock` |
| npm packages | None required by the tool | it is a Rust binary |

### Cargo dependencies

Pinned with `=` in `Cargo.toml` — six Biome crates at exactly `0.5.7`, plus
`serde_json`:

```toml
biome_js_parser  = "=0.5.7"
biome_js_syntax  = "=0.5.7"
biome_rowan      = "=0.5.7"
biome_parser     = "=0.5.7"   # not used directly
biome_diagnostics = "=0.5.7"  # not used directly
biome_console    = "=0.5.7"   # not used directly
serde_json       = "1"
```

The three unused crates are deliberate. `biome_rowan` 0.5.8 is a breaking change
that `biome_js_syntax` 0.5.7 does not compile against, and cargo's semver
unification pulls 0.5.8 in transitively unless every crate in the graph is held at
0.5.7. Listing them is what holds the graph.

**Do not relax these to `^0.5.7` or run `cargo update` on them.** The build breaks.
Reasoning in full: [ARCHITECTURE.md](ARCHITECTURE.md#decision-pinning-all-six-biome-crates-to-057).

`Cargo.lock` is committed — correct for a binary, and load-bearing here given the
pinning.

### What not to install

Once the Biome migration completes, `eslint` and `prettier` come out of
`devDependencies`. Both are still present and still enforced today; removing them
before Biome is wired up leaves the project with no lint coverage at all.
[MIGRATION_NOTES.md](MIGRATION_NOTES.md) has the sequencing, including the eight
`eslint-disable` comments that need translating.

---

## D. Build artefacts

| Path | Size | Keep? |
| --- | --- | --- |
| `target/release/custom-biome-lint` | ~2 MB | Yes — the binary everything invokes |
| `target/` (whole tree) | ~700 MB | No — gitignored, rebuildable |
| `target/debug/` | ~500 MB | No — only `cargo test` needs it |
| `Cargo.lock` | 20 KB | **Yes — committed.** Pins the Biome graph |
| `fixtures/` | small | Yes — tests read them at runtime |
| `docs/`, `src/` | small | Yes |

```sh
# Reclaim space; the release binary must be rebuilt afterwards.
cargo clean

# Or drop just the debug artefacts, keeping the release binary.
rm -rf target/debug
```

`.gitignore` contains `/target`. Add the same to a consumer that vendors or
submodules the tool, so a stray `git add -A` from the parent cannot stage it:

```gitignore
custom-biome-lint/target/
```

Do not delete `Cargo.lock` — "safe to delete, it will regenerate" is true of most
lockfiles and **false here**: regenerating it resolves `biome_rowan` to 0.5.8 and
the build fails.

---

## E. CI artefacts and caching

```yaml
lint:custom:
  stage: test
  image: rust:1.90
  variables:
    CARGO_HOME: ${CI_PROJECT_DIR}/.cargo
  cache:
    # Keyed on Cargo.lock: invalidated only when dependencies change.
    key:
      files:
        - custom-biome-lint/Cargo.lock
    paths:
      - .cargo/
      - custom-biome-lint/target/
  script:
    - cargo build --release --manifest-path custom-biome-lint/Cargo.toml
    - ./custom-biome-lint/target/release/custom-biome-lint src
  artifacts:
    paths:
      - custom-biome-lint/target/release/custom-biome-lint
    expire_in: 30 days
```

- **`CARGO_HOME` inside the project** — GitLab can only cache paths under the
  project directory, so the default `~/.cargo` would not be cached at all.
- **Key on `Cargo.lock`**, not the branch or a static string. A branch key rebuilds
  cold every pipeline; a static key can serve a binary built from older source,
  which silently enforces outdated rules.
- **`expire_in`** keeps a 2 MB binary per pipeline from accumulating. Only publish
  it as an artifact if a later stage consumes it.
- **A cached `target/` does not imply a current binary.** Always run the `cargo
  build` step; cargo decides quickly whether recompilation is needed.

More on the build-vs-cache-vs-vendor trade-off:
[CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md#the-build-step).

---

## Reference

| Topic | Document |
| --- | --- |
| Adding the tool to a project | [INTEGRATION_GUIDE.md](INTEGRATION_GUIDE.md) |
| CI jobs, hooks, rollout | [CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md) |
| Rust install | [SETUP.md](SETUP.md) |
| Why the Biome crates are pinned | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Rule names for `ignoreBiomeExtensionRules` | [RULES.md](RULES.md) |
| Publishing | [PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md) |
