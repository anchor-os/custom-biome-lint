# Publishing custom-biome-lint to npm

Publishing to npm lets consumers install the tool with `yarn add -D` instead of
requiring a Rust toolchain on every machine. That convenience is the entire
argument for doing it.

**Read the [platform caveat](#the-platform-problem-read-this-first) before you
publish anything.** A naive `npm publish` from a Mac ships an arm64 binary that
fails silently on every Linux CI runner.

## The platform problem (read this first)

The package ships a **pre-compiled binary**. `package.json` points `main` and
`bin` at `target/release/custom-biome-lint`, which is whatever `cargo build
--release` last produced on the publishing machine:

```
$ file target/release/custom-biome-lint
target/release/custom-biome-lint: Mach-O 64-bit executable arm64
```

There is no `os`/`cpu` field in `package.json`, so npm will happily install that
arm64 Mach-O binary onto a Linux x86-64 CI runner, where it fails with `Exec
format error` — at lint time, not install time.

Pick one of these before publishing:

| Approach | Effort | Notes |
| --- | --- | --- |
| **Build from source on install** | low | Add a `postinstall` running `cargo build --release`. Requires Rust on every consumer machine — which defeats much of the point, but is honest and always correct. |
| **Guard with `os`/`cpu`** | low | Add `"os": ["darwin"], "cpu": ["arm64"]` so npm *refuses* to install on the wrong platform. Turns a silent runtime failure into a clear install error. Do this at minimum. |
| **Per-platform packages** | high | Publish `custom-biome-lint-darwin-arm64`, `-linux-x64`, etc. as `optionalDependencies` with `os`/`cpu` guards, plus a thin launcher. How esbuild and swc do it. The correct answer for wide distribution. |
| **`cargo-dist`** | medium | Generates cross-platform release artefacts and an installer from CI. Good middle ground once releases are tagged. |

Until one is in place, prefer the submodule route
([USE_AS_GIT_SUBMODULE.md](USE_AS_GIT_SUBMODULE.md)) — each consumer builds for
its own platform and the problem does not arise.

## What actually goes in the tarball

Verify before publishing, always:

```sh
npm pack --dry-run
```

Current output, for reference:

```
package size:  845.6 kB
unpacked size: 2.6 MB
total files:   33
```

The contents are worth understanding, because they are **not** simply the `files`
array:

```json
"files": ["src", "docs", "Cargo.toml", "Cargo.lock", "README.md"]
```

The binary is not listed there, and `.gitignore` contains `/target` — yet
`target/release/custom-biome-lint` (2.4 MB) *is* in the tarball. That is npm
force-including whatever `main` and `bin` point at, regardless of `files` or
ignore rules. Two consequences:

- The package works today only because of that force-include. It is implicit
  behaviour, not an expressed intent — add `"target/release/custom-biome-lint"` to
  `files` so the intent is on the page.
- `fixtures/` is **absent**. Nothing needs it at runtime, but if you want
  consumers to be able to self-test, add it explicitly.

Never publish without reading the `npm pack --dry-run` file list. It is the only
way to know what you are shipping.

## Setup

`package.json` already has the required fields:

```json
{
  "name": "custom-biome-lint",
  "version": "0.1.0",
  "main": "target/release/custom-biome-lint",
  "bin": { "custom-biome-lint": "target/release/custom-biome-lint" }
}
```

Recommended additions before a first publish:

```json
{
  "os": ["darwin"],
  "cpu": ["arm64"],
  "files": [
    "target/release/custom-biome-lint",
    "src",
    "docs",
    "fixtures",
    "Cargo.toml",
    "Cargo.lock",
    "README.md"
  ],
  "repository": {
    "type": "git",
    "url": "https://gitlab.com/your-org/custom-biome-lint.git"
  },
  "engines": { "node": ">=26.4.0" }
}
```

Adjust `os`/`cpu` to match what you actually build, or drop them once you move to
per-platform packages.

### Authenticate

```sh
npm login          # or: npm adduser
npm whoami         # confirm
```

For a scoped, private package — likely the right choice for an internal tool —
name it `@your-org/custom-biome-lint` and publish with `--access restricted`.
Scoped packages are private by default; unscoped ones are always public. Check
which you want before the first publish, because **a published version number can
never be reused**, even after `npm unpublish`.

## Build and publish

```sh
# 1. Build the binary the package points at.
cargo build --release

# 2. Prove it works.
cargo test
./target/release/custom-biome-lint fixtures

# 3. Inspect the tarball.
npm pack --dry-run

# 4. Publish.
npm publish
```

Step 1 is not optional. `npm publish` does not run `cargo build`, so without it
you either ship a stale binary or the publish fails on a missing `bin` target.

Verify:

```sh
npm view custom-biome-lint
npm view custom-biome-lint version
```

## Version bumping

The version lives in **two** files and they must agree — `Cargo.toml` feeds
`--version` output, `package.json` feeds the registry:

```sh
# 1. Edit both.
#    Cargo.toml   -> version = "0.2.0"
#    package.json -> "version": "0.2.0"

# 2. Refresh Cargo.lock with the new version.
cargo build --release

# 3. Commit and tag.
git add Cargo.toml Cargo.lock package.json
git commit -m "chore: release v0.2.0"
git tag -a v0.2.0 -m "v0.2.0"
git push origin main --follow-tags

# 4. Publish (or let CI do it on the tag).
npm publish
```

Avoid `npm version` here: it bumps `package.json` only, leaving `Cargo.toml`
behind, and the drift is invisible until someone compares `--version` output
against the installed package. The CI job in
[EXTRACT_TO_SEPARATE_REPO.md](EXTRACT_TO_SEPARATE_REPO.md) asserts the two match
so a mismatched release fails the pipeline instead of reaching the registry.

See [EXTRACT_TO_SEPARATE_REPO.md](EXTRACT_TO_SEPARATE_REPO.md#versioning) for
what counts as major/minor/patch — note that renaming a rule is breaking, because
it invalidates every suppression comment in every consumer.

## Post-publish verification

Do this in a clean directory, not the source tree — otherwise you are testing your
local build rather than the published artefact:

```sh
mkdir /tmp/verify && cd /tmp/verify
npm install -g custom-biome-lint

custom-biome-lint --help
custom-biome-lint --version      # must match the published version

mkdir -p src
printf 'const cache = new Map();\n' > src/example.js
custom-biome-lint src            # expect: no-native-map violation, exit 1
```

Exit code 1 with one reported violation is success. Exit code 0 means the rules
did not run — most likely the binary is missing from the tarball.

Then check the registry page renders correctly:
`https://www.npmjs.com/package/custom-biome-lint`

## Consuming the published package

```jsonc
{
  "devDependencies": {
    "custom-biome-lint": "^0.1.0"
  },
  "scripts": {
    // Resolved from node_modules/.bin — no path to the binary needed.
    "lint:custom": "custom-biome-lint src"
  }
}
```

This is the one real advantage over the submodule approach: consumers get
`node_modules/.bin/custom-biome-lint` on their `PATH` with no Rust toolchain and
no build step. See [PACKAGE_JSON_SETUP.md](PACKAGE_JSON_SETUP.md) for the full
script set and [INTEGRATION_GUIDE.md](INTEGRATION_GUIDE.md) for CI wiring.

## Caveats summary

- **The published binary is built for one platform.** Guard with `os`/`cpu` at
  minimum; move to per-platform packages for real distribution.
- **CI must build its own binary** for Linux x86-64, separately from the macOS one
  a developer publishes by hand.
- **`main`/`bin` targets are force-included** in the tarball regardless of `files`
  and `.gitignore`. Convenient, but list them explicitly.
- **`Cargo.toml` and `package.json` versions must match.** Nothing enforces this
  locally.
- **Published versions are immutable.** `npm unpublish` frees the name, not the
  version number.
- **Check `npm publish --access`** on the first publish. An internal tool
  published unscoped is public.
