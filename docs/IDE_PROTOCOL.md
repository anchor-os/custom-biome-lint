# IDE Integration Protocol

This document is the contract for editor integrations (VS Code, JetBrains,
language servers, etc.). It covers the **machine-readable outputs** the Rust
binary emits and how an IDE should consume them. The Rust binary is the single
source of truth — editor adapters live outside this repo.

Protocol version: **1** (carried in the top-level `version` field of every JSON
report). Bump the version in `src/cli/output.rs` (`PROTOCOL_VERSION`) only when
a breaking change is required, and keep the prior version documented here.

## 1. Running on a single file

```bash
custom-biome-lint <path> --format json
```

- Accepts a single filepath (or a glob/directory, as before).
- With `--format json`, every violation carries the fields an IDE needs
  (see §3). No `--write-fix`, `--auto-fix`, or `--rules` is implied.
- Exit codes are unchanged: non-zero when error-severity violations are found.
  The JSON report is still written to stdout, so editors should read stdout
  regardless of exit status.

## 2. Rule metadata (`--rules`)

```bash
custom-biome-lint --rules
```

Prints a versioned catalog with no file analysis:

```json
{
  "version": 1,
  "rules": [
    {
      "name": "no-native-map",
      "description": "Disallow the use of native Map in favour of Immutable.js Map",
      "defaultSeverity": "error",
      "enabledByDefault": true,
      "supportedExtensions": ["js", "jsx"]
    }
  ]
}
```

- `defaultSeverity` is one of `error`, `warn`, `off`.
- `enabledByDefault` reflects whether the rule is active under the default
  `PackageConfig` (false for the off-by-default rules; those can still be
  turned on by the user's config).
- Use this endpoint to populate rule lists, severity configuration UI, and
  quick-fix availability without scanning any code.

## 3. Diagnostic shape (`--format json`)

Each `files[].violations[]` entry:

```json
{
  "rule": "no-native-map",
  "message": "Use Immutable.js Map instead of native Map.",
  "severity": "error",
  "line": 1,
  "col": 15,
  "startLine": 1,
  "startColumn": 15,
  "endLine": 1,
  "endColumn": 18,
  "fixes": [ { "kind": "safe", "title": "...", "edits": [ ... ] } ],
  "suppressions": [ { "kind": "suppress", "title": "...", "edits": [ ... ] } ]
}
```

Field rules:

- `line` / `col` — 1-based line, 1-based **byte** column. This is the primary
  location and is always present. (`col` is a byte offset, matching Biome's
  convention; it lines up with editor byte positions, not display columns.)
- `startLine` / `startColumn` — always mirror `line` / `col`. Provided so an
  IDE can read a single stable key regardless of span support.
- `endLine` / `endColumn` — present **only** when the rule tracks a contiguous
  span (e.g. `no-native-map`, `no-arrow-function-create-selector`). Absent
  (omitted from JSON) for line-only rules. When present they are 1-based
  line/byte-column, and `endColumn >= startColumn` on the same line.
- `severity` — `error` or `warning`.
- `fixes` — zero or more **safe** fixes. Currently only `kind: "safe"`. Each
  `Edit` reuses the exact byte range Biome-style autofix would touch, so
  applying the edit is equivalent to running `--auto-fix` for that violation.
- `suppressions` — zero or more suppression edits. Applying an edit adds the
  matching `custom-biome-ignore-line` / `custom-biome-ignore-next-line`
  comment, identical to `--write-fix`.

### `Edit` shape

```json
{
  "startLine": 1, "startColumn": 21,
  "endLine": 1, "endColumn": 21,
  "replacement": " // custom-biome-ignore-line no-native-map"
}
```

- Coordinates are 1-based line, 1-based **byte** column.
- For an insertion, `start` and `end` are equal (zero-width range) and
  `replacement` is the text to insert.
- For a replacement, the `start`..`end` range is replaced by `replacement`.
- An IDE applies edits by converting these byte-column positions to its own
  offset model (typically `byte_offset = line_start + (startColumn - 1)`).

## 4. Severity mapping

| `defaultSeverity` / `severity` | suggested IDE level |
| ------------------------------ | ------------------- |
| `error`                        | Error               |
| `warn`                         | Warning             |
| `off`                          | (rule not active)   |

## 5. stdin support

```bash
cat file.js | custom-biome-lint --stdin <path> --format json
```

- `--stdin` reads source from stdin; `<path>` is required and used only for
  extension/display (it must exist and match an enabled rule's
  `supportedExtensions`). The on-disk file is **not** read when `--stdin` is set.
- Mutually exclusive with `--write-fix`, `--auto-fix`, and `--rules`.

## 6. Compatibility guarantees

- The `version: 1` envelope (`version`, `files`, `summary`) is unchanged for
  pre-existing consumers. New fields (`startLine`/`startColumn`, `end*`,
  `fixes`, `suppressions`) are additive.
- Rule names, severities, and messages are stable across releases unless a
  rule's behavior explicitly changes (a deliberate, version-bumped event).
- The 11 rules and their default severities are not modified by this work;
  `ignoreBiomeExtensionRules` and other config keys are untouched.

## 7. What is intentionally out of scope

LSP servers, daemons, and editor-specific adapters are **not** part of this
repo. This protocol gives editors everything needed to drive the binary as a
push-on-save or on-demand lint step.
