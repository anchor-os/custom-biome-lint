# Distribution: precompiled binaries via npm

`custom-biome-lint` ships as a **main npm package plus eight platform
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

npm's install-time platform resolution means only the platform package
matching the installing machine's `os`/`cpu` (and `libc`, on Linux) actually
downloads; the rest are silently skipped. This is why `npm install
custom-biome-lint` needs no Rust toolchain: no compilation happens on the
consumer's machine at all, only a binary download through the ordinary npm
dependency graph.

Linux ships as two flavors per architecture — glibc and musl (Alpine and most
other musl-based container images) — because a glibc-linked binary cannot run
under musl at all; it fails in the dynamic loader before `main`. See
[Linux: glibc vs musl](#linux-glibc-vs-musl).

This document describes that mechanism. For the source-build path (git
submodule, contributing to this repo), see
[USE_AS_GIT_SUBMODULE.md](USE_AS_GIT_SUBMODULE.md). For the release/publish
procedure, see [PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md).

## Package layout

```
package.json          # main package: name "custom-biome-lint"
bin/cli.js             # launcher — the "bin" entry point npx/yarn run
bin/platform.js        # pure platform+arch+libc -> package-name mapping
npm/
  darwin-arm64/package.json   # name "custom-biome-lint-darwin-arm64"
  darwin-x64/package.json
  linux-arm64/package.json
  linux-arm64-musl/package.json
  linux-x64/package.json
  linux-x64-musl/package.json
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

The two musl packages add `"libc": ["musl"]`, which npm (and pnpm/yarn) use
the same way as `os`/`cpu` — skip this package unless the installing machine's
libc matches. The glibc Linux packages deliberately do *not* declare
`"libc": ["glibc"]`, even though Biome's equivalents do: adding it would make
an install path that works today depend on a field only newer npm/pnpm/yarn
releases honor, and an installer that ignores or misreads it would skip the
package it installs correctly today — a regression for glibc users. The
cost of leaving it off is that a musl machine may download both Linux packages
for its architecture; the launcher still picks the musl one at run time, so
the only loss is a few megabytes of unused download.

No Rust source is duplicated into platform packages — they are produced by
CI copying a compiled binary into `npm/<platform>/bin/` immediately before
publish (see [Release pipeline](#release-pipeline) below). The Rust source of
truth stays in `src/` at the repo root, same as always.

The main `package.json` lists all eight as `optionalDependencies`, pinned to
the exact same version it publishes at:

```json
{
  "optionalDependencies": {
    "custom-biome-lint-darwin-arm64": "0.2.0",
    "custom-biome-lint-darwin-x64": "0.2.0",
    "custom-biome-lint-linux-arm64": "0.2.0",
    "custom-biome-lint-linux-arm64-musl": "0.2.0",
    "custom-biome-lint-linux-x64": "0.2.0",
    "custom-biome-lint-linux-x64-musl": "0.2.0",
    "custom-biome-lint-win32-arm64": "0.2.0",
    "custom-biome-lint-win32-x64": "0.2.0"
  }
}
```

Exact versions, not ranges — a platform package built at 0.2.0 is only
guaranteed compatible with the main package's launcher logic at 0.2.0.
`scripts/set-version.js` is what keeps these eight `optionalDependencies`
entries, the root package's own version, each of the eight
`npm/<platform>/package.json` versions, `package-lock.json`'s copy of all of
that, and `Cargo.toml`'s version in lockstep — see
[Version sync](#version-sync).

## How npm selects the binary

Two layers of platform gating happen, one at install time and one at run
time:

1. **Install time**: npm reads each optional dependency's `os`/`cpu` (and
   `libc`) fields and skips installing any package that doesn't match the
   current machine. Normally one platform package lands in `node_modules`; a
   musl machine may get two, since the glibc packages declare no `libc` (see
   [Package layout](#package-layout)).
2. **Run time**: `bin/cli.js` does its own detection — it doesn't trust that
   npm's install-time gating always ran the way it expects (documented npm
   behavior has changed across versions before, and a lockfile from a
   different platform can be committed and reused). It calls
   `resolvePackageName(process.platform, process.arch, libc)` (in
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
  Linux arm64 (glibc)
  Linux arm64 (musl, e.g. Alpine)
  Linux x64 (glibc)
  Linux x64 (musl, e.g. Alpine)
  Windows arm64
  Windows x64

If you are developing custom-biome-lint from source, use the source-build
workflow documented in docs/USE_AS_GIT_SUBMODULE.md (run `npm run build:native`).
```

On Linux the same message carries a third line, `  libc: musl` (or `glibc`),
because on Linux the libc flavor is part of the resolution key.

It never falls back to `cargo build`, and never downloads anything outside
the npm registry — the platform packages *are* the distribution mechanism.

### Linux: glibc vs musl

A binary linked against glibc does not run on a musl system. On Alpine (by
far the most common musl environment, and a very common CI/Docker base image)
the glibc binary fails before it executes a single instruction of its own
code, with a loader error rather than anything this tool can catch or explain
— typically:

```
Error relocating /path/to/custom-biome-lint: __libc_start_main: symbol not found
```

or, depending on the image, `no such file or directory` from the kernel for a
missing `/lib/ld-linux-x86-64.so.2`. That is why musl gets its own,
statically linked binary and its own package per architecture.

`process.platform` is `"linux"` for glibc and musl alike — there is no
distinct platform value — so `bin/platform.js` detects the libc flavor
separately, in `detectLibc`:

```js
process.report?.getReport()?.header?.glibcVersionRuntime;
```

Node reports the runtime glibc version there when it is linked against glibc,
and omits the field entirely on musl. This is the same probe esbuild, Biome,
and the `detect-libc` package use. Three properties of the implementation
matter:

- **It only applies on Linux.** The field is also absent on macOS and Windows,
  for the ordinary reason that neither has glibc — so treating "absent" as
  "musl" unconditionally would mislabel every macOS machine. `detectLibc`
  returns `null` off Linux and resolution then behaves exactly as it did
  before musl packages existed.
- **An unreadable report resolves to glibc, never musl.** `process.report` is
  optional API surface: it can be missing, replaced, or throw in an embedder
  or a patched runtime. Reading it is wrapped in a `try`, and the
  inconclusive case falls back to `glibc` — the flavor that already worked
  before these packages existed. Guessing `musl` instead would turn a working
  glibc install into a missing-package failure.
- **The libc flavor is passed in, not probed deep inside.** `detectLibc` takes
  its environment and its report reader as parameters with defaults, and
  `resolveBinaryPath` takes `libc` as a parameter, so tests drive every branch
  (musl, glibc, unreadable report, override) from any host without mutating
  process state.

### `CUSTOM_BIOME_LINT_LIBC` escape hatch

Setting `CUSTOM_BIOME_LINT_LIBC=musl` or `CUSTOM_BIOME_LINT_LIBC=glibc`
overrides detection on Linux. It exists for the case detection cannot cover:
a runtime whose `process.report` is stripped or patched, or a distro where
the probe reads the wrong way — without it, the only recourse on such a
machine would be pointing `CUSTOM_BIOME_LINT_BIN` at a binary by hand. Any
other value (including a typo like `MUSL`) is ignored and detection runs
normally, so a mistyped override can never resolve a package name that does
not exist. The variable has no effect off Linux.

The missing-package error names the detected flavor and this variable, so a
machine where detection got it wrong reports its own fix.

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

- Detects `process.platform` / `process.arch`, plus the libc flavor on Linux
  (see [Linux: glibc vs musl](#linux-glibc-vs-musl)).
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
2. **`build`** — a matrix of the eight platform/target pairs. Each leg's first
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
   via `file`. The two musl legs are likewise cross-compiled and unexecuted,
   and are checked for both architecture and static linking (see
   [Linux musl builds](#linux-musl-builds)). `win32-arm64` builds natively on a `windows-11-arm` runner
   (see [Windows ARM64](#windows-arm64) below) and additionally gets a PE-header
   architecture check, since Windows-on-ARM's x64 emulation means a
   successful `--version` run alone doesn't prove the binary is actually
   ARM64. Every leg uploads its binary as a build artifact.
3. **`publish`** — downloads all eight artifacts, re-runs
   `node scripts/set-version.js <version>` (same version, from
   `determine-version`) on its own checkout to sync `Cargo.toml` + all nine
   `package.json` files + `package-lock.json`, then explicitly reads every one
   of those manifests back and asserts it reports the release version, and runs
   `--version` against one staged binary as a final check — before publishing
   anything. It then copies each binary into its `npm/<platform>/bin/` and
   publishes the eight platform packages followed by the main package. The main package
   publishes last because its `optionalDependencies` reference exact
   versions that must already exist on the registry.

### Target triples

| npm platform | Rust target triple | Build host |
| --- | --- | --- |
| `darwin-arm64` | `aarch64-apple-darwin` | `macos-14` (native) |
| `darwin-x64` | `x86_64-apple-darwin` | `macos-15-intel` (native) |
| `linux-arm64` | `aarch64-unknown-linux-gnu` | `ubuntu-22.04` (cross, via `gcc-aarch64-linux-gnu`) |
| `linux-arm64-musl` | `aarch64-unknown-linux-musl` | `ubuntu-22.04` (cross, via `cross`) |
| `linux-x64` | `x86_64-unknown-linux-gnu` | `ubuntu-22.04` (native) |
| `linux-x64-musl` | `x86_64-unknown-linux-musl` | `ubuntu-22.04` (cross, via `cross`) |
| `win32-arm64` | `aarch64-pc-windows-msvc` | `windows-11-arm` (native) |
| `win32-x64` | `x86_64-pc-windows-msvc` | `windows-latest` (native) |

### Linux musl builds

Both musl legs build inside a [`cross`](https://github.com/cross-rs/cross)
container (`cross build --release --target <triple>`), pinned to a released
`cross` binary rather than `cargo install cross`, so a release does not depend
on compiling a build tool from source.

`x86_64-unknown-linux-musl` alone would mostly work with `rustup target add`
plus apt's `musl-tools`. `aarch64-unknown-linux-musl` is the reason for
`cross`: Ubuntu ships no aarch64 *musl* cross-gcc — `gcc-aarch64-linux-gnu`
targets glibc — and pointing the musl target's linker at a glibc cross-gcc is
exactly the class of half-configured cross-linking that already cost this
workflow a rewrite for Windows ARM64. `cross`'s images carry real musl cross
toolchains for both architectures, so both legs use the same mechanism: one
thing to understand, one thing to fix if it breaks.

Neither musl binary is executed in CI. `x86_64-unknown-linux-musl` output is
statically linked and would very likely run on the glibc x64 runner, but it
is built in a container for another libc and treated as cross-compiled
(`can_run: false`) rather than assumed runnable. Instead each leg asserts, via
`file`, that the binary has the expected ELF architecture *and* is statically
linked — static linking is the entire point of these packages, and a musl
binary that came out dynamically linked would fail on Alpine in precisely the
way the glibc binary already does.

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
version, the root `package.json` version and all eight
`optionalDependencies` entries, `package-lock.json`'s two version fields and
its own copy of `optionalDependencies`, and each `npm/<platform>/package.json`
version. Its `PLATFORMS` array is the list every one of those rewrites is
driven from: a platform package added under `npm/` but missing from that array
is silently left at a stale version, and then ships as a package whose version
no longer matches the main package's pin — so its install can never resolve.
Add new platforms there in the same commit.

Run it locally to preview a version bump:

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
entries for the eight platform packages. At bump time those versions are not on
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
- **Neither musl binary is built or run on a musl system.** They are
  cross-compiled in a `cross` container and CI checks only their ELF
  architecture and static linking — no test suite, no fixtures run, no
  `--version` check. A musl-specific runtime bug would not be caught by this
  pipeline; the first real musl execution of a release is a user's.
- **Libc detection is a probe, not a guarantee.** `detectLibc` reads Node's
  process report; a runtime that hides or patches it resolves as glibc, which
  on a musl machine means the launcher reports a missing package rather than
  running. `CUSTOM_BIOME_LINT_LIBC=musl` is the documented way out, and the
  error message says so.
- Every npm trusted publisher (9 packages) must be configured individually
  on npmjs.com against this workflow + the `npm-publish` environment before
  a release can succeed; see [PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md). The two
  musl packages have never been published, so they need the
  [first-time bootstrap](PUBLISH_TO_NPM.md#first-time-bootstrap-a-brand-new-package-name)
  before the first release that includes them.
