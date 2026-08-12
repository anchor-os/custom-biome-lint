# Publishing custom-biome-lint to npm

**Status as of v0.2.0: the package ships precompiled binaries.** `npm
install custom-biome-lint` needs no Rust toolchain — see
[DISTRIBUTION.md](DISTRIBUTION.md) for how the main package, the six
platform packages, and `bin/cli.js` fit together. This document is the
operational runbook: what to run, in what order, to cut a release.

Earlier versions (through v0.2.0's predecessor) shipped a `postinstall`
running `cargo build --release` on the consumer's own machine instead. That
approach is gone; do not reintroduce a `postinstall` compile step.

## The platform problem (background)

A single npm package can carry, at most, one binary per publish. Publishing
a binary built on one machine with no `os`/`cpu` guard means npm will happily
install (say) an arm64 macOS binary onto a Linux x86-64 CI runner, where it
fails with `Exec format error` at lint time, not install time. The fix —
per-platform packages gated by `os`/`cpu`, plus a thin JS launcher that picks
the right one at runtime — is what [DISTRIBUTION.md](DISTRIBUTION.md)
describes and what `.github/workflows/publish.yml` automates. This is the
same shape Biome, esbuild, and swc use.

## Release procedure

Releases are cut from a `v<major>.<minor>.<patch>` git tag. Pushing the tag
triggers `.github/workflows/publish.yml`, which builds all 6 targets and
publishes all 7 packages (see [DISTRIBUTION.md](DISTRIBUTION.md#release-pipeline)
for the job breakdown). The steps below are what that workflow does — useful
both to understand what CI is doing and to reproduce a publish by hand if
CI is ever unavailable.

### 1. Bump the version

```sh
node scripts/set-version.js 0.3.0
```

This writes `0.3.0` into `Cargo.toml`, the root `package.json` (version +
all 6 `optionalDependencies` entries), and every `npm/<platform>/package.json`.
Never hand-edit only one of these — see
[DISTRIBUTION.md#version-sync](DISTRIBUTION.md#version-sync).

```sh
cargo build --release   # refreshes Cargo.lock for the new version
git add Cargo.toml Cargo.lock package.json npm
git commit -m "chore: release v0.3.0"
git tag -a v0.3.0 -m "v0.3.0"
git push origin main --follow-tags
```

Pushing the tag is what starts the release workflow.

### 2. Let CI build and publish

Watch the `Publish to npm` workflow run. It:

1. Derives the release version from the pushed tag once, before anything is
   built (`determine-version` job).
2. Builds all 6 targets in parallel. Each one first re-runs
   `scripts/set-version.js` with that version on its own checkout — so the
   binary it's about to build embeds the release version via
   `env!("CARGO_PKG_VERSION")`, not whatever version happened to be
   committed — then builds, and (native targets) runs tests, the fixtures
   smoke test, and a `--version` check confirming the binary reports that
   exact version.
3. Re-runs `scripts/set-version.js` again on the publish job's own checkout
   (same version), then explicitly verifies every manifest — `Cargo.toml`,
   `package.json`, all 6 `npm/<platform>/package.json` — reports it, and
   runs `--version` against one staged binary, before publishing anything.
4. Stages each built binary into `npm/<platform>/bin/`.
5. Publishes the 6 platform packages, then the main package.

The `npm-publish` GitHub environment gates the `publish` job behind required
reviewer approval — see the comment block at the top of `publish.yml` for
how to configure it, and configure an npm trusted publisher for **each of
the 7 packages** (the main package plus all 6 platform packages) pointing at
this workflow + the `npm-publish` environment. Trusted publishing (OIDC)
means no `NPM_TOKEN` secret is needed or used.

### 3. Verify

```sh
npm view custom-biome-lint version
npm view custom-biome-lint-darwin-arm64 version   # spot-check one platform package
```

Then in a clean directory (not the source tree, so you're testing the
published artifact rather than a local build):

```sh
mkdir /tmp/verify && cd /tmp/verify
npm install custom-biome-lint

npx custom-biome-lint --help
npx custom-biome-lint --version      # must match the tag

mkdir -p src
printf 'const cache = new Map();\n' > src/example.js
npx custom-biome-lint src            # expect: no-native-map violation, exit 1
```

Exit code 1 with one reported violation is success — and confirms Rust was
never required on this machine. Also confirm the registry page renders:
`https://www.npmjs.com/package/custom-biome-lint`.

## Manual publish (CI unavailable)

Only do this if the release workflow itself is broken. It requires a Rust
toolchain matching `RUST_TOOLCHAIN` in `publish.yml`, and **you can only
build for your own machine's platform** — building all 6 by hand across
different machines is what CI exists to avoid. Prefer fixing CI.

**Never publish the main package after only building your own platform.**
The main package's `optionalDependencies` pin all 6 platform packages at
this exact version — publishing it while even one platform package is
missing ships a main package that can never resolve its binary on that
platform, and the version is immutable once published; there is no fixing
it after the fact.

```sh
# 1. Bump versions (see step 1 above), then build and stage *your* platform's
#    binary, e.g. on Apple Silicon macOS:
cargo build --release
cargo test
./target/release/custom-biome-lint fixtures   # prove it works
mkdir -p npm/darwin-arm64/bin
cp target/release/custom-biome-lint npm/darwin-arm64/bin/

# 2. Inspect and publish just that one platform package.
(cd npm/darwin-arm64 && npm pack --dry-run)
(cd npm/darwin-arm64 && npm publish --access public)

# 3. Repeat step 1-2 on a machine for every other platform that isn't
#    already on the registry at this version — or, more realistically,
#    confirm CI's build job already produced and published the rest, and
#    you're only patching the one platform CI's publish job failed on.

# 4. Only once ALL SIX are confirmed present, publish the main package.
for pkg in darwin-arm64 darwin-x64 linux-arm64 linux-x64 win32-arm64 win32-x64; do
  npm view "custom-biome-lint-$pkg@<version>" version || {
    echo "custom-biome-lint-$pkg is missing at <version> — do not publish main yet"
    exit 1
  }
done
npm pack --dry-run   # main package
npm publish --access public
```

Never publish without reading `npm pack --dry-run`'s file list first — it's
the only way to know what's actually going in the tarball.

## Version bumping details

The version lives in **14 fields across 8 files** and they must all agree:
`Cargo.toml`'s version (1), the root `package.json`'s own version (1) plus
its 6 `optionalDependencies` entries (6), and each of the 6
`npm/<platform>/package.json` versions (6). `scripts/set-version.js` is the
only supported way to change it — see
[DISTRIBUTION.md#version-sync](DISTRIBUTION.md#version-sync). Avoid `npm
version`; it only touches the root `package.json` and leaves everything else
behind.

See [EXTRACT_TO_SEPARATE_REPO.md](EXTRACT_TO_SEPARATE_REPO.md#versioning) for
what counts as major/minor/patch — note that renaming a rule is breaking,
because it invalidates every suppression comment in every consumer.

## Consuming the published package

```jsonc
{
  "devDependencies": {
    "custom-biome-lint": "^0.2.0"
  },
  "scripts": {
    // Resolved from node_modules/.bin — no path to the binary needed.
    "lint:custom": "custom-biome-lint src"
  }
}
```

Consumers get `node_modules/.bin/custom-biome-lint` on their `PATH` with no
Rust toolchain and no build step. See
[PACKAGE_JSON_SETUP.md](PACKAGE_JSON_SETUP.md) for the full script set and
[INTEGRATION_GUIDE.md](INTEGRATION_GUIDE.md) for CI wiring.

## Caveats summary

- **Never reintroduce a `postinstall` compile step.** Normal installation
  must not run `cargo build` under any circumstance.
- **Version numbers live in 14 fields across 8 files** and
  `scripts/set-version.js` is the only supported way to change them.
- **Each of the 7 packages needs its own npm trusted publisher** configured
  before the release workflow can publish it.
- **Published versions are immutable.** `npm unpublish` frees the name, not
  the version number — this applies to platform packages too.
- **The Linux ARM64 binary is cross-compiled** and cannot be executed by
  CI — see [DISTRIBUTION.md#limitations](DISTRIBUTION.md#limitations).
  Windows ARM64 builds and runs natively on a `windows-11-arm` runner.
