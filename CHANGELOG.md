# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

## [0.4.2]

### Added
- Loop-statement ban rules, **off by default** (opt in via
  `ignoreBiomeExtensionRules`): `no-for-statement`, `no-while-statement`,
  `no-do-while-statement`. These enforce the "no loops, use functional
  iteration" house style that ESLint's `no-restricted-syntax` provided and
  Biome lacks. `for...of` / `for...in` are deliberately out of scope.
- `param-mutating-array-method-call`, **off by default** — catches
  array-mutating method calls (`push`, `pop`, `shift`, `unshift`, `splice`,
  `sort`, `reverse`, `fill`, `copyWithin`) on a function parameter
  (`param.push(item)`), which the assignment-shaped parameter-mutation rules
  and Biome's `noParameterAssign` do not see.

### Changed
- Biome parser pinned to a git build of **Biome 2.5.8**. This makes
  `$`-prefixed identifiers (e.g. the Cypress `$el` convention) parse cleanly
  instead of emitting a spurious parse-error warning, and brings the
  dependency graph in line with modern Biome.
- README now lists **all 11 rules** with their on/off default and how to toggle
  them (previously only 7 were documented).

### Rule defaults
- **On by default** (`error`): `no-native-map`, `no-arrow-function-create-selector`,
  `reselect-arity-match`, `destructure-default-param-assign`,
  `destructure-param-prop-assign`.
- **Off by default** (opt in): `bare-arrow-param-prop-assign`,
  `deep-param-prop-assign`, `no-for-statement`, `no-while-statement`,
  `no-do-while-statement`, `param-mutating-array-method-call`.

## [0.4.5]

### Fixed
- Hardened the IDE machine-readable contract: `PROTOCOL_VERSION` is now a single
  source of truth in `src/diagnostics/mod.rs`, read by both `--format json` and
  `--rules` (previously two separate hardcoded `1`s). The version number is
  unchanged (`1`), so this is internal-only — no editor adapter changes needed.
- Fixed `docs/IDE_PROTOCOL.md`, which pointed the protocol-version bump at a
  non-existent `PROTOCOL_VERSION` constant.

### Added
- Unicode coordinate regression tests: emoji on the same line before a
  diagnostic (verifies byte vs character column), and production safe-fix /
  suppression edits applied with multi-byte content earlier in the file (verifies
  the byte range stays accurate through an apply → relint cycle).

## [0.4.4]

### Fixed
- Windows CLI JSON regression: a bare Windows drive root now resolves to the
  drive root (`C:/`) instead of a drive-relative path, so `--format json` emits
  valid diagnostics on Windows instead of empty stdout.
- Bare Windows drive-root pattern (`C:/**/*.js`) resolves to the drive root in
  `GlobSet::root_dir`.

### Changed
- Biome parser git rev bumped to the current Biome `main`.

### Added
- Machine-readable IDE protocol (version `1`): `--format json` exposes stable
  `startLine`/`startColumn` (always) plus `endLine`/`endColumn` (span rules),
  `severity`, and structured `fixes` (safe) and `suppressions` suggestions;
  `--rules` exposes the full rule catalog. See `docs/IDE_PROTOCOL.md`.
- README "IDE integration" section documenting the Comment Doc Links extension
  (VS Code + JetBrains/WebStorm) and marketplace links.

## [0.4.3]

### Changed
- Maintenance release: version bump and release/build pipeline alignment.

[0.4.5]: https://github.com/anchor-os/custom-biome-lint/releases/tag/v0.4.5
[0.4.4]: https://github.com/anchor-os/custom-biome-lint/releases/tag/v0.4.4
[0.4.3]: https://github.com/anchor-os/custom-biome-lint/releases/tag/v0.4.3
[0.4.2]: https://github.com/anchor-os/custom-biome-lint/releases/tag/v0.4.2
