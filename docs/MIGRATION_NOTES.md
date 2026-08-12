# Migration notes

Practical state of the ESLint -> Biome migration as it concerns this tool. The
broader migration plan lives in `UI/dashboard/biome-migration.md`.

## Where things stand

| Item | Status |
| --- | --- |
| The 3 custom rules ported to this tool | Done — builds, 42 tests passing, verified against `src/` |
| Positional parity with the old ESLint rules | Verified — 8 findings, 8 pre-existing suppressions, each adjacent |
| 8 `eslint-disable-next-line` comments translated | **Not done** — see below |
| Tool wired into hooks / CI | **Not done** — see [CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md) |
| ESLint removed from the repo | Not done — tracked by the main migration plan |

**Order matters.** Translate the comments *before* wiring the tool into pre-push
or CI. Until then the tool exits 1 against `src/`, which would block every push.

## The 8 comments that need translating

`no-native-map` reports 8 violations in `src/`. Every one sits on the line
immediately after an existing
`// eslint-disable-next-line customPlugin/no-native-map` comment — the ESLint rule
was already suppressing all 8. All 8 are the `new mapboxgl.Map()` false-positive
class, which the port reproduces faithfully (see [RULES.md](RULES.md)).

| # | File | Comment line | Reported violation |
| --- | --- | --- | --- |
| 1 | `src/components/AddExternalEvents/ExternalEventForm.jsx` | 172 | 173:30 |
| 2 | `src/components/CompassDashboardV2/Map.js` | 16 | 17:30 |
| 3 | `src/components/FacilitiesManager/GeofenceEditor.jsx` | 81 | 82:30 |
| 4 | `src/components/FacilitiesManager/LocationPicker.jsx` | 35 | 36:30 |
| 5 | `src/components/MapBuilder/MapCanvas.jsx` | 119 | 120:30 |
| 6 | `src/components/POIManager/tabs/LocationTab.jsx` | 108 | 109:30 |
| 7 | `src/sagas/fetchResourceCapacityForVessel.js` | 98 | 99:24 |
| 8 | `src/sagas/manageSagaCancellation.js` | 10 | 11:25 |

Line numbers were captured on 2026-07-30 against `main`. Re-derive them before
editing rather than trusting this table — any intervening change shifts them:

```sh
cd UI/dashboard
grep -rn "customPlugin/no-native-map" src --include="*.js" --include="*.jsx"
```

### The change

For each of the 8 sites, replace the ESLint comment with this tool's equivalent:

```diff
-    // eslint-disable-next-line customPlugin/no-native-map
+    // custom-biome-ignore-next-line no-native-map
     const map = new mapboxgl.Map({ container, style });
```

Two differences to note:

- **No plugin prefix.** `customPlugin/no-native-map` becomes plain
  `no-native-map`. This tool has a flat rule namespace, so there is nothing to
  qualify.
- **Preserve the indentation.** Six of the eight are indented inside a function
  body; the tool does not care, but the diff should be minimal.

Optionally add a justification, which makes the next reader's life easier given
these are all the same false positive:

```js
// custom-biome-ignore-next-line no-native-map -- mapboxgl.Map, not the native Map
const map = new mapboxgl.Map({ container, style });
```

Text after `--` is ignored by the suppression parser.

### Verifying the change

```sh
cd UI/dashboard
./custom-biome-lint/target/release/custom-biome-lint 'src/**/*.{js,jsx}'; echo "exit=$?"
```

Before: `✖ 8 errors in 8 files`, exit 1.
After: no violations, exit 0.

That clean run is the precondition for making the check blocking in CI.

### On the "9 suppressions" in the migration plan

`biome-migration.md` says `no-native-map` is "actively suppressed in 9 places".
That count is off by one. A repo-wide search finds exactly **8**
`eslint-disable-next-line customPlugin/no-native-map` comments in `src/`, matching
the 8 findings one-for-one. The only other file mentioning the rule name is
`scripts/rebuildEslintDisables.js`, which references it as a string in tooling
rather than suppressing anything — that is presumably where the ninth came from.

**8 is the number.** No stale or orphaned suppression needs hunting down.

## This tool does not read `eslint-disable*` comments

Deliberate, and worth understanding before someone "fixes" it.

The tool recognises only:

```js
// custom-biome-ignore-line <rule>[, <rule2>]
// custom-biome-ignore-next-line <rule>[, <rule2>]
```

An `// eslint-disable-next-line customPlugin/no-native-map` comment is invisible
to it and suppresses nothing.

**Why not just honour both?** It would make the migration a no-op for these 8
sites, which is superficially attractive. But:

1. **Two linters must not both claim one suppression.** While ESLint and this tool
   coexist, an `eslint-disable` comment honoured by both is ambiguous: removing
   the ESLint rule would leave a comment that looks like ESLint's but is actually
   load-bearing for a different tool. Nobody reviewing that file could tell.
2. **Suppressions should name the tool that owns them.** After ESLint is deleted,
   an `eslint-disable` comment that still does something is actively misleading —
   a reader would reasonably assume it is dead and remove it, silently
   reintroducing the violation.
3. **It forces the migration to be explicit.** 8 comments is a small, reviewable,
   one-time cost, and the resulting diff is a clear record that the suppression
   moved from one tool to another. Silent compatibility would leave no such trace.
4. **The prefix does not map cleanly.** ESLint's rule names are plugin-qualified
   (`customPlugin/no-native-map`, `custom/reselect-arity-match` — note the two
   different prefixes in use). Matching them would mean hardcoding prefix
   aliasing rules that exist only to support a tool being deleted.

A `--compat-eslint-disable` flag would be easy to add if the 8-comment change
turns out to be contentious. It is not there because the migration is
deliberately meant to be visible in the diff.

### Translating the 8 comments

`--write-fix` does this mechanically. Preview first, then apply:

```sh
./custom-biome-lint/target/release/custom-biome-lint --write-fix --dry-run src
./custom-biome-lint/target/release/custom-biome-lint --write-fix src
```

On the current tree this adds 8 trailing `// custom-biome-ignore-line no-native-map`
comments across the 8 files and exits 0, after which a plain run reports no
violations. It does **not** remove the adjacent `eslint-disable-next-line`
comments — those stay until the ESLint rules are deleted, and removing them is a
separate, deliberate step.

Note the resulting comment sits *trailing* on the offending line while the ESLint
one sits on the line above, so the two are easy to tell apart in review.

## The other 987 `eslint-disable` comments

The migration plan notes ~987 file-level `/* eslint-disable ... */` comments
across ~950 files, mostly `max-lines`.

**None of them concern this tool.** They target rules this tool does not implement
(`max-lines` has no equivalent here or in Biome), and since the tool ignores
`eslint-disable*` syntax entirely, they are inert as far as it is concerned. They
need no cleanup for this tool's sake. Whether they get cleaned up is a question
for the broader migration.

## The two Reselect rules need no migration

`no-arrow-function-create-selector` and `reselect-arity-match` report **0
violations** across the codebase's 141 `createSelector` call sites. There are no
existing suppressions for either rule, so there is nothing to translate. Both
rules can be enabled with no code changes at all.

Note the fixture files use `custom-biome-ignore-line` markers to exercise suppression
handling, but no real source file needs them.

## Relationship to the earlier `tools/reselect-lint` prototype

An earlier single-purpose prototype exists at `tools/reselect-lint/`. It
implements only `no-arrow-function-create-selector` and uses a different, older
suppression marker:

```js
// reselect-lint-ignore-line
```

`custom-biome-lint` supersedes it: same rule with identical detection logic, plus
the other two rules, configuration, extension filtering, verbosity levels, and
the standardised `custom-biome-ignore-*` suppression syntax.

**The old marker is not recognised by this tool.** No source file uses it — only
`tools/reselect-lint/fixtures/suppressed.js` does — so there is nothing to
translate. Once `custom-biome-lint` is wired into CI, `tools/reselect-lint/`
should be deleted to avoid two tools appearing to own the same rule. Confirm
nothing references it first:

```sh
grep -rn "reselect-lint" --include="*.json" --include="*.yml" --include="*.sh" . | grep -v node_modules
```

## v0.2.0: suppression marker renamed (breaking)

Every suppression comment written with an earlier version of this tool used
`biome-ignore-line` / `biome-ignore-next-line` — the same prefix Biome's own
built-in suppression comments use (`// biome-ignore lint/...`). Since this
tool is meant to run *alongside* Biome on the same files (see
[CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md#how-this-tool-relates-to-biome)),
sharing that prefix was a real collision, not a theoretical one. As of
**v0.2.0** the markers are namespaced:

| Old (pre-0.2.0) | New (0.2.0+) |
| --- | --- |
| `biome-ignore-line` | `custom-biome-ignore-line` |
| `biome-ignore-next-line` | `custom-biome-ignore-next-line` |

**This is a breaking change.** The moment a project upgrades to v0.2.0 or
later, every suppression comment still written with the old marker becomes
invisible to the tool — the violation it was silencing reappears. This is
mechanical to fix and safe to automate, since renaming a marker string never
changes what it means:

```sh
# From the consumer project's root, across whatever extensions this tool lints.
# The `[^-]` right before the marker skips anything already migrated —
# `custom-biome-ignore-*` has a `-` immediately before `biome-ignore`, an old
# unmigrated marker has a space (or `{/* `) there instead — so this is safe
# to run more than once. The `matches=` step guards the case where nothing
# is left to migrate: `xargs sed` with no file list falls through to reading
# stdin and hangs, rather than exiting cleanly.
matches=$(grep -rlE '[^-]biome-ignore-(line|next-line)' src --include="*.js" --include="*.jsx" || true)
if [ -n "$matches" ]; then
  printf '%s\n' "$matches" | xargs sed -i '' -E 's/([^-])biome-ignore-next-line/\1custom-biome-ignore-next-line/g; s/([^-])biome-ignore-line/\1custom-biome-ignore-line/g'
fi
```

(Drop the empty `''` after `-i` on Linux/GNU sed; macOS/BSD sed requires it.)

Do **not** re-run `--write-fix` to "fix" this — the violations were already
suppressed under the old marker, so a plain string rename is correct and
non-destructive; re-running `--write-fix` would instead add a *second*,
redundant suppression comment next to markers it doesn't recognize.

## Checklist

- [ ] Build the binary — [SETUP.md](SETUP.md)
- [ ] Re-derive the 8 comment line numbers with the `grep` above
- [ ] Translate all 8 to `// custom-biome-ignore-next-line no-native-map`
- [ ] Confirm the tool exits 0 against `src/**/*.{js,jsx}`
- [ ] Add the `package.json` scripts — [CI_CD_INTEGRATION.md](CI_CD_INTEGRATION.md)
- [ ] Add the CI job as `allow_failure: true`, observe a few pipelines
- [ ] Make the CI job blocking
- [ ] Add the pre-push hook
- [ ] Delete `tools/reselect-lint/` once nothing references it
- [ ] Correct the "9 suppressions" figure in `biome-migration.md` to 8
