# Installation at source

**This is one alternative installation mode, not the recommended one.**
`custom-biome-lint` is published to the npm registry with precompiled
binaries for all supported platforms (see the root
[README.md](README.md#installation) and
[docs/DISTRIBUTION.md](docs/DISTRIBUTION.md)) — `npm install
custom-biome-lint` needs no Rust toolchain. Use this guide only if you
specifically want to track an unreleased commit directly from the GitHub
repo rather than a published npm version.

Add this dependency to `package.json`, then run yarn/npm install:

```json
"custom-biome-lint": "github:anchor-os/custom-biome-lint#main"
```

(or pin to a commit/tag instead of `#main`, e.g. `#v0.2.0`)

## Rust is still not required here

`bin/cli.js` no longer has a `postinstall` compile step — a `github:` spec
clones the git tree for the **main** package's source, but its
`optionalDependencies` (the eight `custom-biome-lint-<platform>` packages)
still resolve from the npm registry exactly as they would for a normal `npm
install custom-biome-lint`. As long as the commit/branch you're pinning to
declares `optionalDependencies` versions that exist on the registry, `npm
install` fetches the matching precompiled binary and no Rust toolchain is
needed — even though the main package itself came from git, not the
registry.

Because the dependency is fetched via a `github:` spec, npm/yarn also shell
out to the `git` executable to resolve it — make sure `git` is installed
on both local machines and CI/container images.

### If you need a binary that hasn't been released yet

Being ahead of the last published tag does **not**, by itself, mean the
commit references an unpublished version: `.github/workflows/publish.yml`
runs `scripts/set-version.js` only inside its own CI checkouts and never
commits that change back to the repo, so most commits between releases
still declare the *last released* version in their committed
`package.json` — `npm install` on those resolves the already-published
platform binaries just fine, same as installing the npm package normally.

The case that actually needs a source build is a commit whose committed
`package.json` version has been bumped ahead of what's on the registry —
typically a maintainer's own version-bump commit made just before tagging
a release (see [docs/PUBLISH_TO_NPM.md](docs/PUBLISH_TO_NPM.md)), or any
commit that hand-edited the version out of step with a release. That's the
one case where `npm install` won't just work: the optional dependency
fails to resolve, and `npx custom-biome-lint` prints a "could not find its
prebuilt binary package" error at run time rather than failing at install
time (see [DISTRIBUTION.md](docs/DISTRIBUTION.md#how-npm-selects-the-binary)).
If you hit that, you do need Rust — but only to build a binary yourself,
not because installation requires it:

```sh
git clone https://github.com/anchor-os/custom-biome-lint.git
cd custom-biome-lint
npm run build:native   # cargo build --release
```

Then point the launcher at it directly via the `CUSTOM_BIOME_LINT_BIN`
environment variable instead of relying on the optional dependency. A
`git clone` like the one above has no `node_modules/.bin/custom-biome-lint`
entry — nothing installed it — so invoke the launcher script directly
rather than through `npx`:

```sh
CUSTOM_BIOME_LINT_BIN=$(pwd)/target/release/custom-biome-lint \
  node bin/cli.js --help
```

## Usage

```sh
npx custom-biome-lint [pattern] [flags]
```

or add it to `package.json` scripts, e.g.:

```json
"lint:custom": "custom-biome-lint src"
```

Verify the install worked by running:

```sh
npx custom-biome-lint --help
```
