# Setup

Getting from zero to a working binary. Assumes Rust is **not** installed.

## Requirements

| Requirement | Notes |
| --- | --- |
| Rust toolchain | Edition 2021. Any reasonably recent stable `rustc` works; verified on 1.97.1. |
| Disk | ~400 MB for the toolchain, ~300 MB for `target/` after a release build |
| Network | Only for the first build, to fetch crates |
| Node / npm | **Not required.** The `package.json` scripts are conveniences that shell out to cargo. |

## 1. Install Rust

Two options on macOS. Pick one — do not do both, or you will end up with two
`cargo` binaries and confusing `PATH` behaviour.

### Option A: rustup (recommended)

`rustup` is the official installer and the right choice if you might ever need to
switch toolchains or add targets.

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Accept the default installation when prompted. Then load cargo into your current
shell:

```sh
source "$HOME/.cargo/env"
```

The installer appends that line to your shell profile, so new shells pick it up
automatically. If `cargo` is still not found in a new terminal, add it manually
to `~/.zshrc`:

```sh
echo '. "$HOME/.cargo/env"' >> ~/.zshrc
```

Binaries land in `~/.cargo/bin`.

### Option B: Homebrew

Simpler if you already manage everything through brew, but it gives you a single
toolchain with no `rustup` version management.

```sh
brew install rust
```

Binaries land in `/opt/homebrew/bin` (Apple Silicon) or `/usr/local/bin` (Intel).

### Linux

The rustup command above works unchanged. Distro packages also work:

```sh
# Debian/Ubuntu
sudo apt install rustc cargo

# Fedora
sudo dnf install rust cargo
```

Distro packages are often several releases behind; prefer rustup if the build
complains about the toolchain being too old.

## 2. Verify the installation

Both commands must print a version:

```sh
cargo --version
rustc --version
```

Expected shape:

```
cargo 1.97.1 (c980f4866 2026-06-30)
rustc 1.97.1 (8bab26f4f 2026-07-14)
```

Confirm which installation is actually on your `PATH`:

```sh
which cargo rustc
```

`/opt/homebrew/bin/...` means Homebrew; `~/.cargo/bin/...` means rustup. If you
see a mix, or a stale path, restart your shell.

### Troubleshooting

| Symptom | Fix |
| --- | --- |
| `command not found: cargo` | Shell has not loaded cargo's path. Run `source "$HOME/.cargo/env"`, or open a new terminal. |
| `error: could not find 'Cargo.toml'` | You are in the wrong directory. `cd` into `custom-biome-lint/`. |
| `error: package requires rustc 1.x or newer` | Toolchain too old. `rustup update stable`, or `brew upgrade rust`. |
| `linker 'cc' not found` (macOS) | Install Apple's command-line tools: `xcode-select --install`. |
| Network/TLS failure fetching crates | Corporate proxy. Set `HTTPS_PROXY`, or configure a registry mirror in `~/.cargo/config.toml`. |

## 3. Build

From the `custom-biome-lint/` directory:

```sh
cargo build --release
```

Equivalently, if you prefer npm scripts:

```sh
npm run build
```

The first build compiles Biome's parser crates from source and takes roughly
**30 seconds to a few minutes** depending on the machine. Subsequent builds are
incremental and near-instant unless dependencies change.

Expected tail:

```
    Finished `release` profile [optimized] target(s) in 30.73s
```

Warnings from dependency crates are normal. Warnings from `custom-biome-lint`
itself are not — the tool builds clean.

### Why `--release`

A debug build (`cargo build`) works and is fine for iterating on tests, but it is
several times slower to run. Since this tool walks thousands of files and parses
each one, always use `--release` for anything you will actually run against a
codebase or in CI.

### If the build fails on Biome crate versions

Errors mentioning `SyntaxKind::is_trivia`, `SendNode`, or `biome_rowan` version
mismatches mean the dependency pins have been disturbed. All six Biome crates
must stay at exactly `0.5.7`, and `Cargo.lock` must be committed. See the
version-pinning section of [ARCHITECTURE.md](ARCHITECTURE.md) — this is a known,
documented constraint, not a transient failure. Recovery:

```sh
git checkout Cargo.toml Cargo.lock
cargo clean
cargo build --release
```

## 4. Where the binary lands, and how to run it

```
custom-biome-lint/target/release/custom-biome-lint
```

A single self-contained executable with no runtime dependencies — it can be
copied anywhere.

Run it from the directory whose files you want to lint, since patterns are
resolved relative to the working directory. From `UI/dashboard`:

```sh
./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}'
```

From inside `custom-biome-lint/`, against the fixtures:

```sh
./target/release/custom-biome-lint fixtures
```

### Usage

```
custom-biome-lint [PATTERN] [FLAGS]
```

`PATTERN` defaults to `src/**/*.{js,jsx}`. It supports `*`, `?`, `**` and `{a,b}`
brace sets, and a bare directory is expanded for you — `custom-biome-lint src`
means `src/**/*.{js,jsx}`.

**Quote your globs.** Unquoted, the shell expands them first and the tool
receives a list of filenames instead of a pattern, which is an error (only one
positional argument is accepted).

**Patterns must be relative.** An absolute path is joined onto the working
directory rather than used as-is, producing a nonsensical path and exit code 2:

```sh
$ custom-biome-lint /tmp/foo
custom-biome-lint: error: path does not exist: /current/dir/tmp/foo
```

`cd` to the directory you want to lint and use a relative pattern.

```sh
custom-biome-lint                        # default: src/**/*.{js,jsx}
custom-biome-lint 'src/store/**/*.js'    # narrow the scope
custom-biome-lint src/reducers           # bare directory shorthand
custom-biome-lint --help                 # full flag reference
custom-biome-lint --version
```

| Flag | Effect |
| --- | --- |
| `-v`, `--verbose` | Config source, enabled/skipped rules, resolved pattern |
| `-vv` | Brace expansion, walk root, discovery counts |
| `-vvv` | Per-file: rules run, violation count, line count |
| `-d`, `--debug` | Internal state, every step (outranks `-vvv`) |
| `--trace` | Prefix each log line with its source location |
| `-h`, `--help` | Usage |
| `-V`, `--version` | Version |

| Exit code | Meaning |
| --- | --- |
| 0 | No violations |
| 1 | Violations found |
| 2 | Bad usage, or the pattern's root directory does not exist |

Diagnostics go to **stdout**, logging and warnings to **stderr** — so
`custom-biome-lint > report.txt` captures a clean report.

### Optional: put it on your PATH

```sh
cp target/release/custom-biome-lint /usr/local/bin/
```

Or install via cargo, which builds and places it in `~/.cargo/bin`:

```sh
cargo install --path .
```

## 5. Confirm it works

```sh
cargo test                             # expect 157 passing
./target/release/custom-biome-lint fixtures   # expect 52 errors in 10 files, exit 1
```

If both match, the setup is good. [TESTING.md](TESTING.md) covers the full
verification procedure, including running against the real dashboard tree.

## Next steps

- [ARCHITECTURE.md](ARCHITECTURE.md) — how the tool is built and why
- [RULES.md](RULES.md) — what the seven rules catch, their known quirks, and how to opt into the two default-off rules
- [ADDING_A_RULE.md](ADDING_A_RULE.md) — extending it
- [CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md) — wiring it into hooks and pipelines
