# Migration notes

Practical guidance for moving an existing ESLint setup onto this tool.

## This tool does not read `eslint-disable*` comments

Deliberate, and worth understanding before someone "fixes" it.

The tool recognises only:

```js
// custom-biome-ignore-line <rule>[, <rule2>]
// custom-biome-ignore-next-line <rule>[, <rule2>]
```

An `// eslint-disable-next-line <plugin>/<rule>` comment is invisible to it and
suppresses nothing.

### Why not just honour both?

1. **Two linters must not both claim one suppression.** While ESLint and this
   tool coexist, an `eslint-disable` comment honoured by both is ambiguous.
2. **Suppressions should name the tool that owns them.** After ESLint is deleted,
   an `eslint-disable` comment that still does something is actively misleading.
3. **It forces the migration to be explicit.** The translation is a small,
   reviewable, one-time cost, and the resulting diff is a clear record that the
   suppression moved from one tool to another.
4. **The prefix does not map cleanly.** ESLint's rule names are plugin-qualified;
   matching them would mean hardcoding prefix-aliasing rules that exist only to
   support a tool being deleted.

## Translating a suppression

For each site, replace the ESLint comment with this tool's equivalent:

```diff
-    // eslint-disable-next-line some-plugin/no-native-map
+    // custom-biome-ignore-next-line no-native-map
     const map = new mapboxgl.Map({ container, style });
```

Two differences to note:

- **No plugin prefix.** `some-plugin/no-native-map` becomes plain `no-native-map`.
  This tool has a flat rule namespace, so there is nothing to qualify.
- **Preserve the indentation.** The diff should stay minimal.

Optionally add a justification after `--`:

```js
// custom-biome-ignore-next-line no-native-map -- mapboxgl.Map, not the native Map
const map = new mapboxgl.Map({ container, style });
```

Text after `--` is ignored by the suppression parser.

## v0.2.0: suppression marker renamed (breaking)

Every suppression comment written with an earlier version of this tool used
`biome-ignore-line` / `biome-ignore-next-line` — the same prefix Biome's own
built-in suppression comments use. Since this tool is meant to run *alongside*
Biome on the same files, sharing that prefix was a real collision. As of
**v0.2.0** the markers are namespaced:

| Old (pre-0.2.0) | New (0.2.0+) |
| --- | --- |
| `biome-ignore-line` | `custom-biome-ignore-line` |
| `biome-ignore-next-line` | `custom-biome-ignore-next-line` |

**This is a breaking change.** The moment a project upgrades to v0.2.0 or later,
every suppression comment still written with the old marker becomes invisible to
the tool — the violation it was silencing reappears. This is mechanical to fix
and safe to automate, since renaming a marker string never changes what it means:

```sh
matches=$(grep -rlE '[^-]biome-ignore-(line|next-line)' src --include="*.js" --include="*.jsx" || true)
if [ -n "$matches" ]; then
  printf '%s\n' "$matches" | xargs sed -i '' -E 's/([^-])biome-ignore-next-line/\1custom-biome-ignore-next-line/g; s/([^-])biome-ignore-line/\1custom-biome-ignore-line/g'
fi
```

(Drop the empty `''` after `-i` on Linux/GNU sed; macOS/BSD sed requires it.)

Do **not** re-run `--write-fix` to "fix" this — a plain string rename is correct
and non-destructive; re-running `--write-fix` would instead add a second,
redundant suppression comment.

## Relationship to the earlier `tools/reselect-lint` prototype

An earlier single-purpose prototype exists at `tools/reselect-lint/`. It
implements only `no-arrow-function-create-selector` and uses a different, older
suppression marker:

```js
// reselect-lint-ignore-line
```

`custom-biome-lint` supersedes it: same rule with identical detection logic, plus
the other rules, configuration, extension filtering, verbosity levels, and the
standardised `custom-biome-ignore-*` suppression syntax.

**The old marker is not recognised by this tool.** Once `custom-biome-lint` is
wired into CI, `tools/reselect-lint/` should be deleted to avoid two tools
appearing to own the same rule. Confirm nothing references it first:

```sh
grep -rn "reselect-lint" --include="*.json" --include="*.yml" --include="*.sh" . | grep -v node_modules
```

## Checklist

- [ ] Build the binary — [SETUP.md](SETUP.md)
- [ ] Translate existing `eslint-disable` suppressions for ported rules to
      `// custom-biome-ignore-next-line <rule>`
- [ ] Confirm the tool exits 0 against your sources
- [ ] Add the `package.json` scripts — [CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md)
- [ ] Add the CI job as `allow_failure: true`, observe a few pipelines
- [ ] Make the CI job blocking
- [ ] Add the pre-push hook
- [ ] Delete `tools/reselect-lint/` once nothing references it
