# Distribution: precompiled binaries via npm

`custom-biome-lint` ships as a **main npm package plus six platform
packages**, the same shape Biome, esbuild, and swc use:

```
npm install custom-biome-lint
        |
        v
custom-biome-lint (main package: bin/cli.js launcher + package metadata)
        |
        v  optionalDependencies, gated by "os"/"cpu"
custom-biome-lint-<platform>  (e.g. custom-biome-lint-darwin-arm64)
        |
        v
npm/<platform>/bin/custom-biome-lint[.exe]  (precompiled Rust binary)
```

npm's install-time platform resolution means only the **one** platform
package matching the installing machine's `os`/`cpu` actually downloads; the
other five are silently skipped. This is why `npm install custom-biome-lint`
needs no Rust toolchain: no compilation happens on the consumer's machine at
all, only a binary download through the ordinary npm dependency graph.

This document describes that mechanism. For the source-build path (git
submodule, contributing to this repo), see
[USE_AS_GIT_SUBMODULE.md](USE_AS_GIT_SUBMODULE.md). For the release/publish
procedure, see [PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md).

## Package layout

```
package.json          # main package: name "custom-biome-lint"
bin/cli.js             # launcher — the "bin" entry point npx/yarn run
bin/platform.js        # pure process.platform+arch -> package-name mapping
npm/
  darwin-arm64/package.json   # name "custom-biome-lint-darwin-arm64"
  darwin-x64/package.json
  linux-arm64/package.json
  linux-x64/package.json
  win32-arm64/package.json
  win32-x64/package.json
```

Each `npm/<platform>/` directory is published as its own npm package. It
contains nothing but the binary and the metadata needed to gate installation:

```json
{
  "name": "custom-biome-lint-darwin-arm64",
  "version": "0.2.0",
  "os": ["darwin"],
  "cpu": ["arm64"],
  "files": ["bin"]
}
```

No Rust source is duplicated into platform packages — they are produced by
CI copying a compiled binary into `npm/<platform>/bin/` immediately before
publish (see [Release pipeline](#release-pipeline) below). The Rust source of
truth stays in `src/` at the repo root, same as always.

The main `package.json` lists all six as `optionalDependencies`, pinned to
the exact same version it publishes at:

```json
{
  "optionalDependencies": {
    "custom-biome-lint-darwin-arm64": "0.2.0",
    "custom-biome-lint-darwin-x64": "0.2.0",
    "custom-biome-lint-linux-arm64": "0.2.0",
    "custom-biome-lint-linux-x64": "0.2.0",
    "custom-biome-lint-win32-arm64": "0.2.0",
    "custom-biome-lint-win32-x64": "0.2.0"
  }
}
```

Exact versions, not ranges — a platform package built at 0.2.0 is only
guaranteed compatible with the main package's launcher logic at 0.2.0.
`scripts/set-version.js` is what keeps these six `optionalDependencies`
entries, the root package's own version, each of the six
`npm/<platform>/package.json` versions, and `Cargo.toml`'s version — 14
fields across the 7 package manifests plus `Cargo.toml` — in lockstep; see
[Version sync](#version-sync).

## How npm selects the binary

Two layers of platform gating happen, one at install time and one at run
time:

1. **Install time**: npm reads each optional dependency's `os`/`cpu` fields
   and skips installing any package that doesn't match the current machine.
   Only one of the six platform packages actually lands in `node_modules`.
2. **Run time**: `bin/cli.js` does its own detection — it doesn't trust that
   npm's install-time gating always ran the way it expects (documented npm
   behavior has changed across versions before, and a lockfile from a
   different platform can be committed and reused). It calls
   `resolvePackageName(process.platform, process.arch)` (in
   `bin/platform.js`) to get the expected package name, then
   `require.resolve("<package>/package.json")` to find where it actually
   installed, and joins that directory with `bin/custom-biome-lint` (or
   `custom-biome-lint.exe` on `win32`).

If step 2 can't resolve the package — wrong platform, or the optional
dependency failed to install — the launcher prints a specific error instead
of attempting anything else:

```
custom-biome-lint does not have a prebuilt binary for:
  platform: darwin
  architecture: arm64

Supported platforms:
  macOS arm64
  macOS x64
  Linux arm64
  Linux x64
  Windows arm64
  Windows x64

If you are developing custom-biome-lint from source, use the source-build
workflow documented in docs/USE_AS_GIT_SUBMODULE.md (run `npm run build:native`).
```

It never falls back to `cargo build`, and never downloads anything outside
the npm registry — the platform packages *are* the distribution mechanism.

### `CUSTOM_BIOME_LINT_BIN` escape hatch

`bin/cli.js` also honors a `CUSTOM_BIOME_LINT_BIN` environment variable: if
set, it's used directly as the binary path, skipping platform resolution
entirely. This exists for two reasons:

- **Contributors** who ran `npm run build:native` locally can run the JS
  launcher against their freshly built binary without needing a matching
  platform package installed.
- **Tests** (`bin/cli.test.js`) use it to point the launcher at a fixture
  script, so argument/stdio/exit-code forwarding can be verified without a
  real platform package.

## What the launcher does and does not do

`bin/cli.js`:

- Detects `process.platform` / `process.arch`.
- Forwards `process.argv.slice(2)` to the resolved binary **unchanged** — it
  never parses or reinterprets CLI flags. The Rust binary remains the only
  source of truth for argument parsing, `--help`, and `--version` output.
- Forwards stdin/stdout/stderr via `stdio: "inherit"`.
- Forwards the child's exit code, or re-raises its signal on the parent
  process if the child was killed by one.
- Forwards `SIGINT`/`SIGTERM`/`SIGHUP` received by the launcher itself to the
  child process.

It does not retry, does not download anything, and does not attempt to
compile Rust under any circumstance.

## Release pipeline

`.github/workflows/publish.yml` runs on `v*` tags and has three jobs:

1. **`determine-version`** — parses the tag into a `<major>.<minor>.<patch>`
   version and exposes it as a job output. This runs before anything is
   built, and is the *only* place the version is derived from the tag —
   every later job consumes that same output rather than re-deriving it.
2. **`build`** — a matrix of the six platform/target pairs. Each leg's first
   step runs `node scripts/set-version.js <version>`, bumping its own
   checkout's `Cargo.toml` (and the `package.json` files, unused here but
   kept in lockstep by the same script) *before* `cargo build`. This matters
   because the binary embeds its version at compile time via
   `env!("CARGO_PKG_VERSION")` (see `src/cli/mod.rs`) — building before
   syncing the version would bake in whatever version happened to be
   committed, not the tag's version. Native targets (host arch/os matches
   the build target) run `cargo test`, the fixtures smoke test, and a
   `--version` check against the binary they just built, confirming it
   reports the release version exactly. `linux-arm64` remains cross-compiled
   (ubuntu-22.04 x64 host, via `gcc-aarch64-linux-gnu`) and, unable to
   execute its own output, is limited to an ELF-header architecture check
   via `file`. `win32-arm64` builds natively on a `windows-11-arm` runner
   (see [Windows ARM64](#windows-arm64) below) and additionally gets a PE-header
   architecture check, since Windows-on-ARM's x64 emulation means a
   successful `--version` run alone doesn't prove the binary is actually
   ARM64. Every leg uploads its binary as a build artifact.
3. **`publish`** — downloads all six artifacts, re-runs
   `node scripts/set-version.js <version>` (same version, from
   `determine-version`) on its own checkout to sync `Cargo.toml` + all seven
   `package.json` files, then explicitly reads every one of those manifests
   back and asserts it reports the release version, and runs `--version`
   against one staged binary as a final check — before publishing anything.
   It then copies each binary into its `npm/<platform>/bin/` and publishes
   the six platform packages followed by the main package. The main package
   publishes last because its `optionalDependencies` reference exact
   versions that must already exist on the registry.

### Target triples

| npm platform | Rust target triple | Build host |
| --- | --- | --- |
| `darwin-arm64` | `aarch64-apple-darwin` | `macos-14` (native) |
| `darwin-x64` | `x86_64-apple-darwin` | `macos-15-intel` (native) |
| `linux-arm64` | `aarch64-unknown-linux-gnu` | `ubuntu-22.04` (cross, via `gcc-aarch64-linux-gnu`) |
| `linux-x64` | `x86_64-unknown-linux-gnu` | `ubuntu-22.04` (native) |
| `win32-arm64` | `aarch64-pc-windows-msvc` | `windows-11-arm` (native) |
| `win32-x64` | `x86_64-pc-windows-msvc` | `windows-latest` (native) |

### Windows ARM64

`win32-arm64` builds on a native `windows-11-arm` GitHub-hosted runner rather
than cross-compiling `aarch64-pc-windows-msvc` from an x64 runner. Earlier
this cross-compiled from `windows-latest`, relying on `rustup target add`
alone — but that only installs the Rust target; it does not establish the
ARM64 MSVC linker and libraries (the right `INCLUDE`/`LIB`/`PATH`
environment) that cross-linking to `aarch64-pc-windows-msvc` actually needs.
A native runner sidesteps that failure mode entirely: the host's own
toolchain is already ARM64, so there's no cross-linking step to misconfigure.
It also means `win32-arm64` now runs the same native-target checks (tests,
fixtures smoke test, `--version` check) as every other platform except
`linux-arm64` — only the extra PE-header check is `win32-arm64`-specific, kept
because Windows-on-ARM transparently emulates x64 binaries, so a binary
executing successfully isn't on its own proof it's actually ARM64.

### Version sync

`scripts/set-version.js <version>` is the single place version numbers get
written for a release. It updates, in order: `Cargo.toml`'s `[package]`
version, the root `package.json` version and all six
`optionalDependencies` entries, `package-lock.json`'s two version fields and
its own copy of `optionalDependencies`, and each `npm/<platform>/package.json`
version. Run it locally to preview a version bump:

```sh
node scripts/set-version.js 0.3.0
git diff
```

Never hand-edit only one of these files — a mismatch between
`Cargo.toml`'s version (which feeds `--version` output) and the published
npm version is exactly the drift this script exists to prevent. It's also
why the release workflow runs this script *before* building each platform's
binary, not only afterward when staging the npm packages — see
[Release pipeline](#release-pipeline) above.

One thing it deliberately does **not** do: add `package-lock.json`'s resolved
entries for the six platform packages. At bump time those versions are not on
the registry yet, so nothing can resolve them; the lockfile is refreshed as a
separate post-publish step instead. See
[PUBLISH_TO_NPM.md step 4](PUBLISH_TO_NPM.md#4-refresh-package-lockjson-post-publish-required-for-npm-ci)
for why `npm ci` depends on it and how to do it.

## Security

The npm packages are the entire distribution mechanism. Nothing in this
pipeline:

- downloads a binary from anywhere other than the npm registry (no GitHub
  Releases fetch, no curl-to-shell),
- runs a postinstall compilation step,
- executes anything other than the one binary belonging to the platform
  package that matched this machine's `os`/`cpu`.

## Limitations

- **The Linux ARM64 binary is cross-compiled**, not built and tested on real
  ARM64 hardware. CI verifies it exists and has the correct ELF architecture
  header, but cannot run it. Treat a report against that platform with that
  in mind — a bug that only reproduces on real ARM64 hardware could
  theoretically slip through. (Windows ARM64 no longer has this limitation:
  it builds and runs natively on a `windows-11-arm` runner — see
  [Windows ARM64](#windows-arm64).)
- Every npm trusted publisher (7 packages) must be configured individually
  on npmjs.com against this workflow + the `npm-publish` environment before
  a release can succeed; see [PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md).
