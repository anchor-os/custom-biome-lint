# Publishing custom-biome-lint to npm

**Status as of v0.2.0: the package ships precompiled binaries.** `npm
install custom-biome-lint` needs no Rust toolchain — see
[DISTRIBUTION.md](DISTRIBUTION.md) for how the main package, the eight
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
per-platform packages gated by `os`/`cpu`/`libc`, plus a thin JS launcher that
picks the right one at runtime — is what [DISTRIBUTION.md](DISTRIBUTION.md)
describes and what `.github/workflows/publish.yml` automates. This is the
same shape Biome, esbuild, and swc use.

## Release procedure

Releases are cut from a `v<major>.<minor>.<patch>` git tag. Pushing the tag
triggers `.github/workflows/publish.yml`, which builds all 8 targets and
publishes all 9 packages (see [DISTRIBUTION.md](DISTRIBUTION.md#release-pipeline)
for the job breakdown). Steps 1-3 below are what that workflow does — useful
both to understand what CI is doing and to reproduce a publish by hand if
CI is ever unavailable. Step 4 is the one part CI does not do, because it is
only possible after the packages are on the registry.

### 1. Bump the version

```sh
node scripts/set-version.js 0.3.0
```

This writes `0.3.0` into `Cargo.toml`, the root `package.json` (version +
all 8 `optionalDependencies` entries), every `npm/<platform>/package.json`, and
`package-lock.json`'s version fields.
Never hand-edit only one of these — see
[DISTRIBUTION.md#version-sync](DISTRIBUTION.md#version-sync).

```sh
cargo build --release   # refreshes Cargo.lock for the new version
git add Cargo.toml Cargo.lock package.json package-lock.json npm
git commit -m "chore: release v0.3.0"
git tag -a v0.3.0 -m "v0.3.0"
git push origin main --follow-tags
```

Pushing the tag is what starts the release workflow.

### 2. Let CI build and publish

Watch the `Publish to npm` workflow run. It:

1. Derives the release version from the pushed tag once, before anything is
   built (`determine-version` job).
2. Builds all 8 targets in parallel. Each one first re-runs
   `scripts/set-version.js` with that version on its own checkout — so the
   binary it's about to build embeds the release version via
   `env!("CARGO_PKG_VERSION")`, not whatever version happened to be
   committed — then builds, and (native targets) runs tests, the fixtures
   smoke test, and a `--version` check confirming the binary reports that
   exact version.
3. Re-runs `scripts/set-version.js` again on the publish job's own checkout
   (same version), then explicitly verifies every manifest — `Cargo.toml`,
   `package.json`, all 8 `npm/<platform>/package.json` — reports it, and
   runs `--version` against one staged binary, before publishing anything.
4. Stages each built binary into `npm/<platform>/bin/`.
5. Publishes the 8 platform packages, then the main package.

The `npm-publish` GitHub environment gates the `publish` job behind required
reviewer approval — see the comment block at the top of `publish.yml` for
how to configure it, and configure an npm trusted publisher for **each of
the 9 packages** (the main package plus all 8 platform packages) pointing at
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

### 4. Refresh `package-lock.json` (post-publish, required for `npm ci`)

`scripts/set-version.js` keeps the lockfile's *version numbers* in step with
`package.json`, but it cannot add the resolved dependency entries for the eight
platform packages: at bump time those versions do not exist on the registry
yet — this release is what publishes them. Until they are resolved, the
lockfile lists all eight `optionalDependencies` without any matching entry, and
`npm ci` refuses the tree:

```
npm error `npm ci` can only install packages when your package.json and
npm error package-lock.json ... are in sync.
npm error Missing: custom-biome-lint-darwin-arm64@ from lock file
```

So regenerate it once the packages are actually on the registry — after step 2
has published and step 3 has verified them, never before:

```sh
git switch main && git pull
npm install --package-lock-only    # now resolvable: every platform package exists
git add package-lock.json
git commit -m "chore: refresh package-lock.json for v0.3.0"
git push
```

`--package-lock-only` writes the lockfile without installing anything, so this
does not depend on the current machine's platform. Run it on a clean `main`
checkout so the commit contains nothing but the lockfile.

Verify before committing — all eight entries should now be present and at the
released version. This exits non-zero on any problem, so it can gate a script
rather than relying on reading the output:

```sh
node -e 'const l = require("./package-lock.json");
  const root = l.packages[""];
  console.log("root:", root.version);
  let bad = 0;
  for (const [name, want] of Object.entries(root.optionalDependencies)) {
    // Lockfile v2/v3 key packages by install path, not by bare name.
    const entry = l.packages[`node_modules/${name}`];
    const state = !entry ? "MISSING" : entry.version !== want ? `MISMATCH (${entry.version})` : "ok";
    if (state !== "ok") bad++;
    console.log(" ", name, want, state);
  }
  if (bad) { console.error(`${bad} entr${bad === 1 ? "y" : "ies"} not resolved at the released version`); process.exit(1); }
  console.log("all", Object.keys(root.optionalDependencies).length, "platform entries resolved");'
```

`MISSING` means that platform package is not on the registry — go back to step 2
rather than committing a lockfile that codifies the gap. `MISMATCH` means the
lockfile resolved an older version than the one being released, which usually
means `npm install --package-lock-only` ran before the publish completed.

**Why this is a separate step and not part of the release commit:** it is the
one piece of version bookkeeping that is only *possible* after publishing, so
folding it into step 1 would either fail (unresolvable versions) or silently
record the previous release's entries. Skipping it is not fatal — the package
installs fine via `npm install`, which is how consumers get it — but it leaves
`npm ci` broken for anyone who vendors this repo, and it is why the lockfile
drifted from `0.2.0` to `0.2.1` unnoticed before v0.3.0.

## First-time bootstrap: a brand-new package name

This is a **different problem from "CI is broken"** — the release workflow can be working
perfectly and still be unable to publish, the first time a package name has never existed on the
registry before. It happened on this project's actual first `v0.2.0` release: the `build` job
succeeded across all 6 platforms (there were 6 at the time), but `publish` failed at the very
first `npm publish` call with:

```
npm http fetch POST 404 .../oidc/token/exchange/package/custom-biome-lint-darwin-arm64
npm verbose oidc Failed token exchange request with body message: OIDC token exchange error - package not found
npm error code ENEEDAUTH
```

**Why:** this workflow publishes with npm Trusted Publishing (OIDC) — no `NPM_TOKEN`, no classic
auth. A trusted publisher can only be configured from a package's *existing* npmjs.com settings
page. There is no equivalent of PyPI's "pending publisher" for a name that has never been
published — confirmed via npm's own docs and the still-open feature request
[npm/cli#8544, "Allow publishing initial version with OIDC"](https://github.com/npm/cli/issues/8544).
So for a genuinely new package name, OIDC has nothing to attach to yet, and the very first publish
has to happen a different way. This bites every platform package the first time it ships, and the
main package too if it's ever renamed.

**This applies right now to `custom-biome-lint-linux-x64-musl` and
`custom-biome-lint-linux-arm64-musl`.** Neither name has ever been published, so the release that
first includes them will fail in the `publish` job on those two packages unless they are
bootstrapped first, by the procedure below. The other 7 packages are unaffected — their trusted
publishers already exist.

**The one-time fix**, run once per new package name, never needed again once its trusted publisher
is configured:

Only ever run it for names that have **never** been published. Republishing a
version that already exists is an error, not a no-op — so the commands below
name the two musl packages specifically rather than looping over all nine.

```sh
# 1. Get real, CI-built binaries from the build job that already succeeded
#    (no need to build them locally):
gh run download <build-run-id> --repo anchor-os/custom-biome-lint --dir /tmp/bootstrap-artifacts

# 2. Stage each into its platform package (both are unix, so both need chmod +x):
for platform in linux-x64-musl linux-arm64-musl; do
  mkdir -p "npm/$platform/bin"
  cp "/tmp/bootstrap-artifacts/binary-$platform/custom-biome-lint" "npm/$platform/bin/"
  chmod +x "npm/$platform/bin/custom-biome-lint"
done

# 3. Publish with your own npm login (npm will likely prompt for browser-based
#    one-time-password auth — that's expected and can only be completed by a human,
#    not from an unattended/automated shell):
(cd npm/linux-x64-musl && npm publish --access public)
(cd npm/linux-arm64-musl && npm publish --access public)
```

Do **not** publish the main package here, and do not touch the seven packages
that already exist: the normal release sequence handles those, and the main
package must publish last so its `optionalDependencies` pin versions that are
already on the registry. For a future new package name, substitute it above.

No `--provenance` flag here — that requires OIDC/CI attestation, which a local human-run publish
doesn't have. Provenance starts applying from the next CI-driven release onward.

**Then, for each package that just got its first publish**, go to its npmjs.com Settings page →
Trusted Publisher, and configure it (GitHub Actions, `anchor-os/custom-biome-lint`,
`.github/workflows/publish.yml`, environment `npm-publish`) — same configuration
[step 2 of the release procedure](#2-let-ci-build-and-publish) already assumes exists. Once every
package that's ever going to be published has a trusted publisher configured, this entire section
never applies again — every subsequent version bump goes through `publish.yml` with zero manual
`npm publish` calls, for every package, forever.

## Manual publish (CI unavailable)

Only do this if the release workflow itself is broken. It requires a Rust
toolchain matching `RUST_TOOLCHAIN` in `publish.yml`, and **you can only
build for your own machine's platform** — building all 8 by hand across
different machines is what CI exists to avoid. Prefer fixing CI.

**Never publish the main package after only building your own platform.**
The main package's `optionalDependencies` pin all 8 platform packages at
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

# 4. Only once ALL EIGHT are confirmed present, publish the main package.
for pkg in darwin-arm64 darwin-x64 linux-arm64 linux-arm64-musl linux-x64 linux-x64-musl \
           win32-arm64 win32-x64; do
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

The version lives in **28 fields across 11 files** and they must all agree:
`Cargo.toml`'s version (1), the root `package.json`'s own version (1) plus
its 8 `optionalDependencies` entries (8), each of the 8
`npm/<platform>/package.json` versions (8), and `package-lock.json`'s two root
versions (2) plus its own copy of the 8 `optionalDependencies` entries (8).
`scripts/set-version.js` is the
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
- **Version numbers live in 28 fields across 11 files** and
  `scripts/set-version.js` is the only supported way to change them.
- **`package-lock.json` needs a post-publish refresh** (step 4) for `npm ci`
  to work. The resolved platform-package entries cannot exist before the
  release that publishes them, so this is the one version-bookkeeping step
  the release workflow cannot do.
- **Each of the 9 packages needs its own npm trusted publisher** configured
  before the release workflow can publish it. The two musl packages do not
  have one yet — see
  [first-time bootstrap](#first-time-bootstrap-a-brand-new-package-name).
- **Published versions are immutable.** `npm unpublish` frees the name, not
  the version number — this applies to platform packages too.
- **The Linux ARM64 and both musl binaries are cross-compiled** and cannot be
  executed by CI — see
  [DISTRIBUTION.md#limitations](DISTRIBUTION.md#limitations). Windows ARM64
  builds and runs natively on a `windows-11-arm` runner.
