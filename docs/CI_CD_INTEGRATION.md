# CI/CD integration

**Status: guidance only — none of this has been applied yet.** The tool builds,
passes its tests, and runs correctly against the dashboard tree, but it is not
wired into any hook or pipeline. This document is the plan for doing that.

Before wiring it in, complete the suppression-comment migration described in
[MIGRATION_NOTES.md](MIGRATION_NOTES.md) — until those 8 comments are translated,
the tool exits 1 against `src/`, which would block every push.

## How this tool relates to Biome

**It runs alongside Biome, not inside it.**

Biome 2.5.5 has **no plugin system**. There is no way to register a custom rule
with Biome and have `biome check` execute it. That is the entire reason this tool
exists as a separate binary: the three custom ESLint rules had nowhere to go
inside Biome.

So the pipeline runs two independent checks:

```
biome check .              -> formatting + Biome's built-in lint rules
custom-biome-lint 'src/**' -> the 3 Reselect/Redux/Immutable rules Biome lacks
```

Each has its own exit code, and both must pass. Keeping them separate is
deliberate: the tool's findings are attributable and easy to reason about, and
neither check can mask the other's failures.

What this tool does share with Biome is the **parser**. It is built on
`biome_js_parser`, so it sees the same AST Biome does — same JSX handling, same
tolerance for JSX inside `.js` files. There is no risk of the two tools
disagreeing about what the source code means.

### Forward-looking: if Biome ships a plugin API

**This is not a current capability.** A plugin API has been discussed for a
future Biome release (rumoured for Biome 3.0), but nothing is available today and
no timeline is committed.

If and when it lands, these rules are unusually well-positioned to port into it:
they are already written against Biome's own `biome_js_syntax` node types, so the
detection logic — the hard part — would transfer largely unchanged. The work
would be replacing this crate's `Rule` trait, `FileContext`, file discovery,
suppression parsing, and output formatting with Biome's equivalents, while the
per-rule `check` bodies mostly survive.

Treat that as a possible future simplification, not a plan. Until it exists, the
standalone-binary approach is the only option, and it works.

## The build step

The binary must exist before anything can call it. Unlike a Node tool, there is
no `npx` that fetches and runs it — cargo must compile it first. Three options:

| Approach | Build cost | Tradeoffs |
| --- | --- | --- |
| **Build in CI** | ~30s–2min cold, near-zero warm with caching | Simplest, always matches source. Needs Rust in the CI image. |
| **Cache the binary** | Build once per dependency change | Fast, but cache invalidation must be keyed correctly or you run a stale binary. |
| **Vendor the binary** | Zero | No Rust needed in CI, but commits a platform-specific artifact and needs one per target OS/arch. Not recommended. |

**Recommendation: build in CI with a cargo cache.** It keeps source and binary in
lockstep, which matters for a linter — a stale binary silently enforcing outdated
rules is worse than a slow build.

Local development needs the same one-time step:

```sh
cd UI/dashboard/custom-biome-lint && cargo build --release
```

Developers without Rust installed will need it — see [SETUP.md](SETUP.md). This is
a real cost of the approach and worth stating explicitly to the team before
making the hook mandatory.

## `package.json` script

Add a script so every caller uses one definition rather than repeating the path:

```json
{
  "scripts": {
    "lint:custom": "./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}'",
    "lint:custom:build": "cargo build --release --manifest-path custom-biome-lint/Cargo.toml"
  }
}
```

A guard that builds the binary if it is missing avoids a confusing
"no such file" error for anyone who has not built it yet:

```json
{
  "scripts": {
    "lint:custom": "test -x ./custom-biome-lint/target/release/custom-biome-lint || yarn lint:custom:build; ./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}'"
  }
}
```

Note this only checks existence, not freshness. In CI, run the build explicitly.

## `.husky/pre-push`

The hook currently reads:

```sh
yarn eslint && yarn prettier:check
```

After the Biome migration it becomes something like `yarn biome:check`. Add the
custom check as a **separate command**, chained with `&&`:

```sh
#!/usr/bin/env sh
. "$(dirname -- "$0")/_/husky.sh"

yarn biome:check && yarn lint:custom
```

With a clearer failure message:

```sh
#!/usr/bin/env sh
. "$(dirname -- "$0")/_/husky.sh"

set -e

echo "Running Biome check..."
yarn biome:check

echo "Running custom lint rules..."
if ! yarn lint:custom; then
  echo ""
  echo "Custom lint rules failed. See UI/dashboard/custom-biome-lint/docs/RULES.md"
  echo "To suppress a specific finding: // biome-ignore-next-line <rule-name>"
  exit 1
fi
```

Keep it on **pre-push, not pre-commit**. The tool lints a whole glob rather than a
staged-file list, so on a large tree it is too slow for the commit path. Biome
handles the fast staged-file formatting in `pre-commit`.

If you do want a pre-commit variant, pass the staged files as the pattern — but
note the tool accepts only **one** positional argument, so a file list will not
work as-is. That would need a loop:

```sh
for f in $(git diff --cached --name-only --diff-filter=ACM | grep -E '\.(js|jsx)$'); do
  ./custom-biome-lint/target/release/custom-biome-lint "$f" || exit 1
done
```

This re-invokes the binary per file, losing the batching benefit. Prefer pre-push.

## `.gitlab-ci.yml`

The `lint` and `lintmr` jobs currently both run:

```yaml
lint:
  stage: test and build stage
  script:
  - yarn eslint
  - yarn prettier:check
  except:
    - tags@hornblower/dashboard

lintmr:
  stage: test and build stage
  script:
    - yarn eslint
    - yarn prettier:check
  only:
    - merge_requests
```

### Option A: add a step to the existing jobs

Smallest change. Reuses whatever `before_script`/cache the jobs already have, but
requires Rust in the job's image.

```yaml
lint:
  stage: test and build stage
  script:
  - yarn biome:check
  - yarn lint:custom:build
  - yarn lint:custom
  except:
    - tags@hornblower/dashboard

lintmr:
  stage: test and build stage
  script:
    - yarn biome:check
    - yarn lint:custom:build
    - yarn lint:custom
  only:
    - merge_requests
```

### Option B: a dedicated job (recommended)

Runs in parallel with the Biome job, fails independently so the MR shows exactly
which check broke, and can use a Rust image without touching the Node jobs.

```yaml
.custom_lint_template: &custom_lint
  stage: test and build stage
  image: rust:1-slim
  cache:
    key:
      files:
        - UI/dashboard/custom-biome-lint/Cargo.lock
    paths:
      - UI/dashboard/custom-biome-lint/target/
      - .cargo/
  variables:
    CARGO_HOME: "$CI_PROJECT_DIR/.cargo"
  before_script:
    - cd UI/dashboard/custom-biome-lint
    - cargo build --release
    - cd ..
  script:
    - ./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}'

custom-lint:
  <<: *custom_lint
  except:
    - tags@hornblower/dashboard

custom-lint-mr:
  <<: *custom_lint
  only:
    - merge_requests
```

Key points:

- **Cache keyed on `Cargo.lock`.** The dependency graph only changes when that
  file changes, so this is the correct invalidation key. Do not key on branch or
  commit — you would rebuild every pipeline.
- **`CARGO_HOME` inside the project directory.** GitLab can only cache paths
  under `$CI_PROJECT_DIR`, and the default `~/.cargo` is outside it. Without this
  the crate downloads are not cached.
- **`cargo build --release`, never debug.** Debug builds are several times slower
  at runtime.
- **Adjust paths** if your `.gitlab-ci.yml` already runs from `UI/dashboard`.

### Verifying the CI setup

The tool's exit codes are what CI keys on:

| Code | Meaning | CI result |
| --- | --- | --- |
| 0 | No violations | pass |
| 1 | Violations found | fail |
| 2 | Bad usage, or pattern root missing | fail — **investigate the config, not the code** |

Exit code 2 deserves attention: it usually means the working directory is wrong
or the glob was shell-expanded before reaching the tool. A misconfigured job that
silently matches no files would otherwise exit **0** and give a false green — the
worst outcome for a linter. Confirm the job actually inspects files by checking
the `-v` output on first setup:

```yaml
script:
  - ./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}' -vv
```

`-vv` reports directories scanned and file counts on stderr. Once you have
confirmed a non-zero file count, drop back to the quiet form.

**Always quote the glob** in YAML. Unquoted, the shell expands it and the tool
receives many positional arguments, which is a usage error.

## Recommended rollout order

1. **Build and test locally** — [SETUP.md](SETUP.md), [TESTING.md](TESTING.md).
2. **Migrate the 8 suppression comments** —
   [MIGRATION_NOTES.md](MIGRATION_NOTES.md). Confirm the tool then exits 0
   against `src/`.
3. **Add the `package.json` scripts.**
4. **Add the CI job in non-blocking form first** (`allow_failure: true`) and let
   it run for a few pipelines to confirm it behaves and to gauge build time.
5. **Make it blocking** — remove `allow_failure`.
6. **Add the pre-push hook last**, once CI has proved stable. A broken hook blocks
   every developer immediately, whereas a broken CI job only blocks merges.

```yaml
# Step 4: observe before enforcing
custom-lint-mr:
  <<: *custom_lint
  allow_failure: true
  only:
    - merge_requests
```

## Optional: disabling a rule in CI

If a rule proves too noisy, disable it by name in
`UI/dashboard/package.json` rather than removing the whole check:

```json
{
  "ignoreBiomeExtensionRules": ["no-native-map"]
}
```

The other rules keep running. Under `-v` the tool reports which rules were
skipped and why, so a disabled rule is visible rather than silently absent.
