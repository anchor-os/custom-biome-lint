# Extracting custom-biome-lint to its own repository

The tool currently lives inside the <PRIVATE_REPO> repo at `custom-biome-lint/`. It has
no dependency on the <PRIVATE_REPO> — it is a self-contained Cargo project — so it can
be lifted out as-is.

Do this when a second project needs the tool. Until then, the in-repo copy is
simpler: one clone, one CI pipeline, no version skew.

## Prerequisites

- GitLab account with permission to create a project in the target group
- `git` on the command line
- Rust toolchain (see [SETUP.md](SETUP.md)) to verify the extracted copy builds

## 1. Copy the directory out

Copy to a location **outside** the <PRIVATE_REPO> working tree, so the parent repo's
`.git` and `.gitignore` are not inherited:

```sh
cp -R \
  /path/to/<PRIVATE_REPO>/custom-biome-lint \
  ~/src/custom-biome-lint

cd ~/src/custom-biome-lint
```

Drop the build output — it is 700+ MB of machine-specific artefacts and is
gitignored anyway:

```sh
rm -rf target
```

Confirm the copy stands alone before going further:

```sh
cargo test          # 88 unit + 68 integration + 1 doc-test
cargo clippy --all-targets
cargo build --release
./target/release/custom-biome-lint fixtures
```

If that passes outside the <PRIVATE_REPO> tree, there are no hidden dependencies.
[TESTING.md](TESTING.md) covers this portability check in more detail.

## 2. Create the GitLab project

The <PRIVATE_REPO> lives at `<private-<PRIVATE_REPO>-repo>`, so the natural home
is the same group:

```
<this-repo>
```

Create it through the GitLab UI (**New project → Create blank project**) with the
README and CI templates **disabled** — the repo already has both. Substitute your
own group below if you are putting it elsewhere.

## 3. Initialise and push

```sh
cd ~/src/custom-biome-lint
git init
git add .
git commit -m "feat: initial extraction of custom-biome-lint from <PRIVATE_REPO>"
git remote add origin git@gitlab.com:<this-repo>.git
git push -u origin main
```

Check `git status` before committing: `target/` must not appear. The `.gitignore`
already contains `/target`.

## 4. Fix the repository metadata

`Cargo.toml` currently claims a GitHub URL that does not exist:

```toml
repository = "https://github.com/anchor/custom-biome-lint"
```

Correct it to the real remote, and add the same field to `package.json`:

```toml
repository = "https://<this-repo>"
```

```json
{
  "repository": {
    "type": "git",
    "url": "https://<this-repo>.git"
  }
}
```

Stale metadata here is not cosmetic — npm renders it as the package's source link,
and `cargo publish` rejects unreachable URLs.

## 5. Add CI

Create `.gitlab-ci.yml`. This mirrors the gates in [TESTING.md](TESTING.md):

```yaml
stages:
  - test
  - build
  - publish

# Cargo's registry and the build directory dominate CI time. Keyed on
# Cargo.lock so the cache is invalidated only when dependencies change.
cache:
  key:
    files:
      - Cargo.lock
  paths:
    - .cargo/
    - target/

variables:
  CARGO_HOME: ${CI_PROJECT_DIR}/.cargo
  CARGO_TERM_COLOR: always

test:
  stage: test
  image: rust:1.90
  script:
    - cargo fmt --check
    - cargo clippy --all-targets -- -D warnings
    - cargo test

build:
  stage: build
  image: rust:1.90
  script:
    - cargo build --release
    - ./target/release/custom-biome-lint fixtures
  artifacts:
    paths:
      - target/release/custom-biome-lint
    expire_in: 30 days

publish-npm:
  stage: publish
  image: node:26.4.0
  script:
    # Fail early if the two version numbers have drifted apart.
    - test "$(node -p "require('./package.json').version")" = "$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
    - npm config set //registry.npmjs.org/:_authToken "${NPM_TOKEN}"
    - npm publish
  rules:
    - if: $CI_COMMIT_TAG
```

Two things worth pinning down rather than copying blindly:

- **`-D warnings`** promotes Clippy warnings to errors. The tool is currently
  warning-free; this keeps it that way.
- **`rust:1.90`** — pin a version rather than `rust:latest` so a new compiler
  release cannot break the pipeline unannounced. The tool builds on 1.90+; it is
  developed against 1.97.
- **`NPM_TOKEN`** must be added as a masked, protected CI/CD variable
  (**Settings → CI/CD → Variables**). Never commit it.

The `publish-npm` job only runs on tags. See [PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md)
for the packaging caveats — in particular, a binary built in CI is Linux x86-64
and will not run on a developer's Mac.

## 6. Tag a release

Versions live in **two** files and must match: `Cargo.toml` and `package.json`.

```sh
# Bump both files to 0.1.0 first, then:
git add Cargo.toml package.json
git commit -m "chore: release v0.1.0"
git tag -a v0.1.0 -m "v0.1.0"
git push origin main --follow-tags
```

Consumers then pin by tag, whether as a submodule
([USE_AS_GIT_SUBMODULE.md](USE_AS_GIT_SUBMODULE.md)) or an npm dependency.

## Versioning

Semantic versioning, where the "API" is **what the tool reports**:

| Change | Bump | Why |
| --- | --- | --- |
| Bug fix that removes false positives | patch | `0.1.0 → 0.1.1` |
| New rule, or a new CLI flag | minor | `0.1.0 → 0.2.0` — additive, but new findings can fail a previously green build |
| Rule renamed or removed; existing rule reports materially more | major | `0.1.0 → 1.0.0` — breaks suppression comments and `ignoreBiomeExtensionRules` entries |

Renaming a rule is a **breaking** change even though no code depends on it: every
`// custom-biome-ignore-line <old-name>` in every consumer silently stops working. If you
must rename, keep the old name accepted for one minor release.

Stay on `0.x` until the rule set has settled.

## README for the extracted repo

The existing `README.md` is already standalone and points into `docs/`. Two edits
after extraction:

1. Replace <PRIVATE_REPO>-relative paths with repo-relative ones.
2. Add a line near the top explaining where it came from and who consumes it:

   > Extracted from the Hornblower <PRIVATE_REPO>, where it runs alongside Biome to
   > cover Reselect/Redux patterns Biome does not implement. See
   > `docs/INTEGRATION_GUIDE.md` to add it to a project.

Keep the substance in `docs/` rather than duplicating it in the README — the docs
travel with the repo.

## Optional: GitHub mirror

Only worth it if you intend to open-source the tool. GitLab can push-mirror
automatically (**Settings → Repository → Mirroring repositories**) with the GitHub
URL and a personal access token as the password.

Mirror in one direction only. A two-way mirror on a repo people actually commit to
produces conflicts that are tedious to unpick.

## After extraction: the <PRIVATE_REPO> side

The <PRIVATE_REPO> now needs to consume the tool rather than contain it. Pick one:

- **Submodule** — [USE_AS_GIT_SUBMODULE.md](USE_AS_GIT_SUBMODULE.md)
- **npm dependency** — [PUBLISH_TO_NPM.md](PUBLISH_TO_NPM.md)

Then follow [INTEGRATION_GUIDE.md](INTEGRATION_GUIDE.md) to wire up scripts, CI,
and the pre-push hook. Do not delete `custom-biome-lint/` from the <PRIVATE_REPO> until
the replacement runs green in CI.
