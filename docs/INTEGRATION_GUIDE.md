# Integration guide

How to add custom-biome-lint to a project that does not already contain it.

## Which document you want

This guide covers the tool arriving from **outside** the consuming repo — as a
submodule, a local copy, or an npm package — and the first-time developer setup
that follows.

| Situation | Read |
| --- | --- |
| Tool already lives in the dashboard at `custom-biome-lint/` | [CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md) — CI jobs, hooks, rollout order |
| Adding it to a new project | this document |
| Just need the scripts and fields | [PACKAGE_JSON_SETUP.md](PACKAGE_JSON_SETUP.md) |
| Replacing ESLint rules | [MIGRATION_NOTES.md](MIGRATION_NOTES.md) |

Paths differ between setups, and mixing them up is the most common integration
error. In the dashboard the tool is at `custom-biome-lint/`; the examples below
assume an external tool at `tools/custom-biome-lint/`. Adjust one or the other —
do not copy both.

---

## A. Setup: three options

### Option 1: submodule (recommended for multiple projects)

One upstream repo, each consumer pinned to a tag. Full walkthrough in
[USE_AS_GIT_SUBMODULE.md](USE_AS_GIT_SUBMODULE.md).

```sh
git submodule add \
  git@gitlab.com:hornblower/custom-biome-lint.git \
  tools/custom-biome-lint
git commit -m "chore: add custom-biome-lint as a submodule"
```

Requires Rust on every machine and submodule handling in CI.

### Option 2: local copy

Simplest for a single project, and the right choice while the rule set is still
changing.

```sh
cp -R /path/to/custom-biome-lint tools/custom-biome-lint
rm -rf tools/custom-biome-lint/target        # 700+ MB of build output
git add tools/custom-biome-lint
git commit -m "chore: vendor custom-biome-lint"
```

No submodule ceremony; updates are a manual re-copy. Fine until a second project
needs the tool, at which point the copies drift.

### Option 3: npm package

No Rust required — consumers get `node_modules/.bin/custom-biome-lint`.

```sh
yarn add -D custom-biome-lint
```

**Read the platform caveat in [PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md) first.** The
published package currently carries a single pre-built binary; installing it on a
platform it was not built for fails at lint time, not install time. Options 1 and
2 avoid this entirely.

---

## B. Build and install

Rust is a one-time install per machine (~10 min). Full instructions, including
Homebrew and Linux package managers, in [SETUP.md](SETUP.md).

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version        # 1.90 or newer
```

Build the tool:

```sh
cd tools/custom-biome-lint
cargo build --release
cd ../..
```

The first build takes 1–3 minutes (Biome's parser crates dominate); rebuilds are
seconds. The binary lands at `tools/custom-biome-lint/target/release/custom-biome-lint`
and is about 2 MB.

Verify:

```sh
./tools/custom-biome-lint/target/release/custom-biome-lint --version
cd tools/custom-biome-lint && ./target/release/custom-biome-lint fixtures
```

The fixtures run should report violations from each rule's `invalid.js` and exit 1.
That is the tool working correctly — see [TESTING.md](TESTING.md).

---

## C. package.json

Add scripts so no caller hardcodes the path:

```json
{
  "scripts": {
    "lint:custom": "test -x tools/custom-biome-lint/target/release/custom-biome-lint || yarn lint:custom:build; tools/custom-biome-lint/target/release/custom-biome-lint src",
    "lint:custom:build": "cargo build --release --manifest-path tools/custom-biome-lint/Cargo.toml",
    "lint:custom:fix": "tools/custom-biome-lint/target/release/custom-biome-lint src --write-fix",
    "lint:custom:dry": "tools/custom-biome-lint/target/release/custom-biome-lint src --write-fix --dry-run"
  }
}
```

The `test -x || build` guard turns a confusing "no such file or directory" into an
automatic first build. It checks **existence, not freshness** — in CI, run
`lint:custom:build` explicitly so a cached binary cannot outlive its source.

`--write-fix` does not rewrite offending code. It inserts
`// biome-ignore-line <rule>` comments, which is how you adopt the tool on an
existing codebase without a large refactor. Always inspect
`yarn lint:custom:dry` output first.

Configure which rules are enforced from the **consumer's** root `package.json`:

```json
{
  "ignoreBiomeExtensionRules": ["no-native-map"]
}
```

The tool reads the nearest `package.json` at or above the linted files. Full field
reference in [PACKAGE_JSON_SETUP.md](PACKAGE_JSON_SETUP.md).

---

## D. CI/CD

[CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md) covers the build-vs-cache trade-off,
the recommended rollout order, and how to make a rule non-blocking while you
observe it. Read it — this section only covers what an *external* tool adds.

The dashboard's `.gitlab-ci.yml` uses `image: node:24.11.0` with a `lint` job that
runs `yarn eslint && yarn prettier:check`. Since the image has no Rust, add a
dedicated job rather than bolting cargo onto the Node one:

```yaml
lint:custom:
  stage: test
  image: rust:1.90
  # Only needed for Option 1 (submodule).
  variables:
    GIT_SUBMODULE_STRATEGY: recursive
    GIT_SUBMODULE_DEPTH: 1
    CARGO_HOME: ${CI_PROJECT_DIR}/.cargo
  cache:
    key:
      files:
        - tools/custom-biome-lint/Cargo.lock
    paths:
      - .cargo/
      - tools/custom-biome-lint/target/
  script:
    - cargo build --release --manifest-path tools/custom-biome-lint/Cargo.toml
    - ./tools/custom-biome-lint/target/release/custom-biome-lint src
```

Three things that matter:

- **Cache key on `Cargo.lock`**, not the branch. Keyed wrongly, you get either a
  cold build every pipeline or a stale binary enforcing outdated rules — the
  second is worse, and silent.
- **`GIT_SUBMODULE_STRATEGY`** is required for Option 1 and harmless otherwise.
  Without it the directory is empty and the build fails confusingly.
- **Separate job** keeps the Rust toolchain out of the Node jobs and lets the
  linter run in parallel with them.

With Option 3 (npm) none of this applies — the binary is in `node_modules`, so
`yarn lint:custom` runs in the existing Node job with no Rust and no extra
configuration.

---

## E. Pre-push hook

The dashboard's `.husky/pre-push` currently reads:

```sh
yarn eslint && yarn prettier:check
```

Chain the custom check with `&&`:

```sh
yarn eslint && yarn prettier:check && yarn lint:custom
```

[CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md#huskypre-push) has the version with
proper failure messaging, and explains why this belongs on **pre-push rather than
pre-commit** — the tool lints a glob rather than a staged-file list, and accepts
only one positional pattern.

Do not make the hook mandatory before CI is green on the same rules. A hook that
blocks pushes for findings the team has not yet agreed to fix gets bypassed with
`--no-verify`, and then it protects nothing.

---

## F. What not to commit

After building:

```gitignore
# Consumer's .gitignore
tools/custom-biome-lint/target/
```

`target/` reaches ~700 MB. The submodule has its own `/target` ignore rule, but
adding it to the consumer protects against a stray `git add -A` from the parent
repo.

Safe to delete at any time — all of it rebuilds:

```sh
rm -rf tools/custom-biome-lint/target/debug     # debug artefacts, ~500 MB
cargo clean --manifest-path tools/custom-biome-lint/Cargo.toml   # everything
```

Do **not** delete `tools/custom-biome-lint/.git` for a submodule — it is a file
pointing into the parent's `.git/modules`, and removing it detaches the submodule.
`Cargo.lock` must also stay: it pins the six Biome crates to `=0.5.7`, and the
build genuinely breaks without it (see
[ARCHITECTURE.md](ARCHITECTURE.md#decision-pinning-all-six-biome-crates-to-057)).

---

## G. First-time developer setup

The sequence to put in the project's contributing guide:

```sh
# 1. Clone with submodules (Option 1; plain clone otherwise).
git clone --recurse-submodules git@gitlab.com:hornblower/dashboard.git
cd dashboard

# Already cloned without them?
git submodule update --init --recursive

# 2. Install Rust if needed (one-time, ~10 min).
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# 3. Install Node dependencies (installs husky hooks via `prepare`).
yarn

# 4. Build the linter.
yarn lint:custom:build

# 5. Check it works.
yarn lint:custom
```

Step 5 exiting non-zero on an existing codebase is expected, not a broken setup —
it means the rules found real violations. Decide as a team whether to fix them or
baseline them with `yarn lint:custom:fix`, then adopt the hook.

---

## Verifying the integration

```sh
# The tool runs and reports its version.
yarn lint:custom -- --version

# It finds a known violation.
mkdir -p src/scratch && printf 'const c = new Map();\n' > src/scratch/probe.js
yarn lint:custom                      # expect a no-native-map violation, exit 1
rm -rf src/scratch

# A suppression comment silences it.
mkdir -p src/scratch
printf 'const c = new Map(); // biome-ignore-line no-native-map\n' > src/scratch/probe.js
yarn lint:custom                      # expect exit 0
rm -rf src/scratch
```

If the first probe exits 0, the rules are not running — check that the pattern
argument reaches the binary and that the rule is not listed in
`ignoreBiomeExtensionRules`. Run with `-vv` to see the resolved pattern, the
config file found, and which rules are enabled:

```sh
yarn lint:custom -- -vv
```

Verbosity levels are documented in [TESTING.md](TESTING.md).

## Reference

| Topic | Document |
| --- | --- |
| Rust install, per-platform | [SETUP.md](SETUP.md) |
| CI jobs, hooks, rollout order | [CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md) |
| package.json fields and scripts | [PACKAGE_JSON_SETUP.md](PACKAGE_JSON_SETUP.md) |
| Submodule workflow | [USE_AS_GIT_SUBMODULE.md](USE_AS_GIT_SUBMODULE.md) |
| npm packaging and its caveats | [PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md) |
| What each rule reports | [RULES.md](RULES.md) |
| Replacing ESLint equivalents | [MIGRATION_NOTES.md](MIGRATION_NOTES.md) |
