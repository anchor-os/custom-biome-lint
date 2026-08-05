# Using custom-biome-lint as a git submodule

A submodule pins a consumer repo to an exact commit of the linter. Each project
builds the binary for its own platform, which sidesteps the cross-platform
packaging problem described in [PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md).

This assumes the tool has already been extracted to its own repo — see
[EXTRACT_TO_SEPARATE_REPO.md](EXTRACT_TO_SEPARATE_REPO.md).

## When this is the right choice

Use a submodule when the tool is shared across two or more projects and every
developer already has, or can install, Rust.

Prefer a **local copy** for a single project (see
[INTEGRATION_GUIDE.md](INTEGRATION_GUIDE.md#option-2-local-copy)) and an **npm
package** when consumers must not need a Rust toolchain.

## Add the submodule

```sh
cd /path/to/consuming-repo

git submodule add \
  git@gitlab.com:your-org/custom-biome-lint.git \
  tools/custom-biome-lint

git commit -m "chore: add custom-biome-lint as a submodule"
```

Two files are staged: `.gitmodules` and a special entry for
`tools/custom-biome-lint` that records a **commit SHA**, not the files themselves.

Use the SSH URL for a private repo — an HTTPS submodule prompts for credentials on
every CI clone.

### Pin to a tag

By default the submodule tracks whatever commit you added. To follow a release
tag instead:

```sh
cd tools/custom-biome-lint
git checkout v0.1.0
cd ../..
git add tools/custom-biome-lint
git commit -m "chore: pin custom-biome-lint to v0.1.0"
```

Recording the tag in `.gitmodules` makes `--remote` updates follow releases rather
than `main`:

```ini
[submodule "tools/custom-biome-lint"]
	path = tools/custom-biome-lint
	url = git@gitlab.com:your-org/custom-biome-lint.git
	branch = v0.1.0
```

Pinning to a tag is strongly preferable to tracking `main`. A linter that changes
underneath you turns an unrelated PR red.

## Clone a repo that has the submodule

```sh
git clone --recurse-submodules git@gitlab.com:your-org/your-app.git
```

If someone already cloned without it — the submodule directory will be present but
empty:

```sh
git submodule update --init --recursive
```

This is the single most common submodule complaint. Make it the first line of the
project's setup instructions, and have the build script fail loudly when the
directory is empty rather than reporting a confusing missing-binary error.

## Update to a newer version

```sh
# Fetch and check out the latest of the tracked branch/tag.
git submodule update --remote tools/custom-biome-lint

# Rebuild — the source changed, the binary has not.
cd tools/custom-biome-lint && cargo build --release && cd ../..

# Run the consumer's lint to see what the new version reports.
yarn lint:custom

# Record the new pin.
git add tools/custom-biome-lint
git commit -m "chore: bump custom-biome-lint to v0.2.0"
```

The diff for that commit is a one-line SHA change. Put the linter version in the
commit message, because the SHA alone tells a reviewer nothing.

Always run the consumer's lint **before** committing the bump. A new version may
introduce a rule that reports existing code, and you want to discover that now
rather than in someone else's pipeline.

## Build and run

```sh
cd tools/custom-biome-lint
cargo build --release
cd ../..

# Lint the consumer's source.
./tools/custom-biome-lint/target/release/custom-biome-lint src
```

Wire it into `package.json` so nobody types that path:

```json
{
  "scripts": {
    "lint:custom": "tools/custom-biome-lint/target/release/custom-biome-lint src",
    "build:lint-tool": "cd tools/custom-biome-lint && cargo build --release"
  }
}
```

Full script set in [PACKAGE_JSON_SETUP.md](PACKAGE_JSON_SETUP.md).

### Configuration still comes from the consumer

`ignoreBiomeExtensionRules` is read from the **nearest `package.json` at or above
the linted files** — so the consumer's root `package.json`, not the submodule's:

```json
{
  "ignoreBiomeExtensionRules": ["no-native-map"]
}
```

The submodule ships the rules; the consumer decides which are enforced.

## CI

CI must fetch submodules explicitly. GitLab:

```yaml
variables:
  GIT_SUBMODULE_STRATEGY: recursive
  GIT_SUBMODULE_DEPTH: 1
```

GitHub Actions:

```yaml
- uses: actions/checkout@v4
  with:
    submodules: recursive
```

Both need credentials for a private submodule. On GitLab, `GIT_SUBMODULE_STRATEGY`
works out of the box for repos in the same group; across groups, add a deploy key
or use a relative URL in `.gitmodules`.

CI also needs Rust and should cache `target/` — see
[INTEGRATION_GUIDE.md](INTEGRATION_GUIDE.md#d-cicd) and
[CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md).

## Ignore the build output

The submodule's own `.gitignore` covers `/target` for the submodule's repo. Add it
to the **consumer's** `.gitignore` too, so a stray `git add -A` from the parent
cannot stage 700 MB of build artefacts:

```gitignore
tools/custom-biome-lint/target/
```

## Trade-offs

**In favour**

- One source of truth across projects; a rule fix propagates by bumping a pin.
- Versioning via git tags, with each consumer pinned to an exact commit.
- Each consumer builds for its own platform — no cross-compilation, no
  platform-specific packages.
- Full source is present, so debugging a false positive means reading the rule.

**Against**

- Every developer needs Rust (~10 min one-time install, see [SETUP.md](SETUP.md)).
- A build step before first lint, and again after every bump.
- Extra git ceremony: `--recurse-submodules` on clone, `--remote` on update, and
  the familiar failure mode of an empty submodule directory.
- CI must be configured for submodules, and needs credentials if the repo is
  private.
- Submodule commits are easy to forget: updating the submodule without committing
  the parent's pointer leaves teammates on the old version.

If the last two points sound like recurring support requests, publish to npm
instead and let consumers install a binary.

## Alternative: git subtree

`git subtree` copies the files into the parent repo rather than referencing them,
so clones need no special flags and nothing can be left uninitialised — at the cost
of a much noisier history and a fiddlier update path. Worth knowing about if
submodule friction becomes the dominant complaint, but a submodule pinned to a tag
is the more conventional choice.
