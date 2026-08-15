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

[0.4.2]: https://github.com/anchor-os/custom-biome-lint/releases/tag/v0.4.2
