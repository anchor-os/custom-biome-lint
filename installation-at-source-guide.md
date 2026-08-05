# Installation at source

Since this is a public GitHub repo and the package builds from source on
install (no npm registry, no auth token needed), install it directly via a
git dependency — the simplest path for a public repo.

Add this dependency to `package.json`, then run yarn/npm install:

```json
"custom-biome-lint": "github:anchor-os/custom-biome-lint#main"
```

(or pin to a commit/tag instead of `#main` once a release exists, e.g.
`#v0.1.0`)

This is a Rust CLI wrapped as an npm package. Installing it will run a
postinstall step (`cargo build --release`) — the Rust toolchain (`cargo`)
must be available on the machine/CI image doing the install. Because the
dependency is fetched via a `github:` spec, npm/yarn also shell out to the
`git` executable to resolve it — make sure `git` is installed alongside
`cargo`, on both local machines and CI/container images.

After install, run it via:

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
