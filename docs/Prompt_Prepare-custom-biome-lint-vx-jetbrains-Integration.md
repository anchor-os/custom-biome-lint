# Prepare `custom-biome-lint` for IDE Integration

We need to prepare the Rust-based `custom-biome-lint` repository for integration into our existing VS Code + JetBrains extension.

Repository:

`https://github.com/anchor-os/custom-biome-lint`

## Goal

Do **not** implement the VS Code or JetBrains integration in this repository.

Instead, improve `custom-biome-lint` so it exposes a clean, stable, machine-readable contract that an IDE extension can consume for:

1. Diagnostics
2. Rule/severity information
3. Safe fixes
4. Suppression fixes

The Rust linter must remain the **single source of truth** for rule behavior.

The existing 11 rules, their detection logic, defaults, configuration behavior, messages, and suppression semantics must not be changed unless absolutely required for the new IDE contract.

---

## Existing rules

Keep the current behavior exactly as-is:

### Enabled by default

- `no-native-map`
- `no-arrow-function-create-selector`
- `reselect-arity-match`
- `destructure-default-param-assign`
- `destructure-param-prop-assign`

### Disabled by default

- `bare-arrow-param-prop-assign`
- `deep-param-prop-assign`
- `no-for-statement`
- `no-while-statement`
- `no-do-while-statement`
- `param-mutating-array-method-call`

The existing `package.json` configuration using:

```json
{
  "ignoreBiomeExtensionRules": {
    "no-native-map": "off",
    "reselect-arity-match": "warn"
  }
}
```

must remain the source of truth.

Do not move rule configuration into the IDE.

---

# 1. Stable machine-readable diagnostics — ✅ done

Review the existing `--format json` output.

Make sure the JSON schema is stable and suitable for an IDE.

At minimum every violation must expose:

```json
{
  "path": "src/example.js",
  "line": 12,
  "col": 30,
  "severity": "error",
  "rule": "no-native-map",
  "message": "..."
}
```

Preserve the existing JSON fields and compatibility guarantees.

If changes are required, make them additive rather than breaking.

Document the schema clearly.

The IDE must be able to reliably determine:

- file
- start line
- start column
- severity
- rule ID
- message

If the current implementation has enough information to provide an exact diagnostic range, expose it as additional fields:

```json
{
  "startLine": 12,
  "startColumn": 30,
  "endLine": 12,
  "endColumn": 37
}
```

Use a clearly documented coordinate convention.

Do not silently change the meaning of existing `line`/`col`.

---

# 2. Add machine-readable fix information — ✅ done

The current CLI supports:

```text
--auto-fix
```

but an IDE needs to know what edit can be applied without asking the IDE to execute a whole-file CLI auto-fix.

Add an optional `fixes` field to violations where a safe, deterministic fix exists.

Example:

```json
{
  "line": 12,
  "col": 30,
  "severity": "error",
  "rule": "some-rule",
  "message": "...",
  "fixes": [
    {
      "kind": "safe",
      "title": "Apply safe fix",
      "edits": [
        {
          "startLine": 12,
          "startColumn": 30,
          "endLine": 12,
          "endColumn": 37,
          "replacement": "..."
        }
      ]
    }
  ]
}
```

Requirements:

- Do not invent fixes for rules that don't have a safe/unambiguous fix.
- Rules without fixes simply omit `fixes` or return an empty array.
- Never label an unsafe or ambiguous transformation as `safe`.
- Support multiple edits in one fix if necessary.
- Preserve the existing CLI `--auto-fix` behavior.
- Ideally reuse the same internal fix generation logic for CLI auto-fix and IDE fix output so they cannot diverge.

Do not implement IDE-specific logic inside individual rules.

---

# 3. Add machine-readable suppression fixes — ✅ done

The IDE must be able to offer:

```text
Suppress <rule>
```

for a diagnostic.

The Rust tool already understands:

```js
// custom-biome-ignore-next-line no-native-map
```

and:

```js
const cache = new Map(); // custom-biome-ignore-line no-native-map
```

Reuse the existing suppression logic.

Expose a suppression fix through the machine-readable output.

For example:

```json
{
  "kind": "suppress",
  "title": "Suppress no-native-map",
  "edits": [
    {
      "startLine": 11,
      "startColumn": 1,
      "endLine": 11,
      "endColumn": 1,
      "replacement": "// custom-biome-ignore-next-line no-native-map\n"
    }
  ]
}
```

The exact edit representation can differ if the existing source architecture suggests a better design.

Important:

**The IDE must not implement suppression comment placement itself.**

The Rust tool owns suppression semantics.

It must correctly handle:

- JS
- JSX
- comments
- JSX children
- line length/placement rules
- existing suppression comments
- rule-specific suppression
- existing suppression syntax

Reuse the logic already used by `--write-fix`.

---

# 4. Rule metadata — ✅ done (`--rules`)

Expose stable metadata for all rules.

The IDE will eventually need to display things such as:

```text
no-native-map

Use Immutable.js Map instead of native Map.

Default severity: error
```

Create a machine-readable rule metadata representation containing at least:

```json
{
  "name": "no-native-map",
  "description": "...",
  "defaultSeverity": "error"
}
```

For off-by-default rules:

```json
{
  "name": "no-for-statement",
  "description": "...",
  "defaultSeverity": "off"
}
```

The metadata should come from the Rust rule registry rather than being duplicated in a separate hard-coded CLI/IDE table.

If possible, expose it through a CLI command such as:

```bash
custom-biome-lint --rules
```

or:

```bash
custom-biome-lint --format json --rules
```

Choose the API that best fits the existing CLI architecture.

Document it.

---

# 5. IDE-friendly single-file operation — ✅ done

Review the current CLI behavior and make sure the IDE can efficiently lint one file.

The IDE will frequently need to do:

```text
lint this exact JS/JSX file
```

without accidentally linting the entire workspace.

Support an explicit file path:

```bash
custom-biome-lint /absolute/path/to/file.js --format json
```

and ensure the JSON response remains clean and parseable.

No logs/debug output should pollute stdout when JSON mode is active.

Existing documentation says diagnostics go to stdout and logging goes to stderr. Preserve this guarantee.

---

# 6. Stdin support — evaluate, don't force it — ✅ done (`--stdin`)

Evaluate whether supporting:

```bash
custom-biome-lint --stdin --format json
```

would materially improve IDE integration.

If it can be added cleanly, implement it.

If not, do not add unnecessary complexity.

The IDE will initially be able to provide the file path, so stdin is optional for this phase.

Do not introduce a long-running server/LSP implementation yet.

---

# 7. Version the machine-readable protocol — ✅ done (see `docs/IDE_PROTOCOL.md`)

The existing JSON response already contains:

```json
{
  "version": 1
}
```

Preserve this.

Treat the JSON format as an API contract.

Document:

- version semantics
- required fields
- optional fields
- severity values
- fix structure
- edit coordinate convention
- rule metadata structure

Future additions should be backward-compatible whenever possible.

---

# 8. Tests — ✅ done (`tests/ide_contract.rs` + `docs/IDE_PROTOCOL.md`)

Add comprehensive tests for the new contract.

At minimum test:

## Diagnostics

- error
- warn
- off
- default severity
- package.json override

### Locations

- correct line
- correct column
- multi-line violation if applicable
- exact diagnostic range where available

### Fixes

- safe fix exists
- no safe fix
- multiple edits
- fix produces expected source

### Suppression

- line suppression
- next-line suppression
- JSX suppression
- named rule suppression
- multiple rule suppression
- existing suppression

### Rule metadata

Verify all 11 rules are exposed with:

- correct rule name
- description
- correct default severity

### Compatibility

Existing tests must continue passing.

Run:

```bash
cargo test
npm test
npm run test:js
cargo clippy
```

as applicable to the current repository.

Do not weaken existing tests.

---

# 9. Important architectural constraint

Do NOT create separate implementations such as:

```text
Rust fix logic
VS Code fix logic
JetBrains fix logic
```

The desired architecture is:

```text
                 custom-biome-lint
                       Rust
                        │
        ┌───────────────┼────────────────┐
        │               │                │
   Diagnostics        Fixes         Suppressions
        │               │                │
        └───────────────┼────────────────┘
                        │
                  JSON contract
                        │
              ┌─────────┴─────────┐
              │                   │
          VS Code             JetBrains
          adapter              adapter
```

The IDE integrations should only translate the Rust output into their native editor APIs.

---

# 10. Do NOT implement these yet — N/A (intentionally not implemented; constraints honored)

Do not:

- add VS Code code
- add JetBrains/Kotlin code
- add LSP
- add a long-running daemon
- duplicate rule implementations
- change existing rule behavior
- change default rule severities
- change `ignoreBiomeExtensionRules`
- automatically download binaries
- introduce a new configuration system

This task is only to make the Rust linter **IDE-ready**.

---

# 11. Final deliverables

At the end provide:

1. Files changed
2. Explanation of the new machine-readable diagnostic schema
3. Example JSON response containing:
   - error
   - warning
   - safe fix
   - suppression fix
4. Rule metadata example
5. Exact CLI commands an IDE can use
6. Documentation location for the protocol
7. Test results
8. Any limitations or decisions that should be addressed before implementing the VS Code adapter

Most importantly:

**Do not over-engineer this.**

We currently have only 11 custom rules. Build the smallest clean contract that lets a VS Code and JetBrains adapter consume the Rust linter reliably while keeping Rust as the single source of truth.