# Bug: `--write-fix` can emit a suppression Biome's own formatter later breaks

Discovered during real-world use: rolling out `bare-arrow-param-prop-assign`
and `deep-param-prop-assign` on the `hornblower/UI/dashboard` monorepo (218
findings, `--write-fix` then a full `biome check --write src cypress` format
pass). Of 218 auto-placed suppressions, **9 were silently detached from their
violation by the formatter pass**, plus one case where `--write-fix`'s own
output and a pre-existing `biome-ignore` comment broke *each other's*
adjacency requirement. None of this produced a diagnostic anywhere — the only
way it surfaced was re-running `custom-biome-lint` after the format pass and
noticing violations that should have been gone.

This is the same *class* of bug as the `no-native-map` / mapboxgl.Map false
positive documented in the dashboard repo's own
`.vscode/vite-for-dashboard/rebase-and-biome-triage-guide.md` (gotcha #4): a
suppression comment that is textually correct at the moment it's written, but
whose correctness depends on an assumption (here: "the formatter will not
move this token") that isn't checked.

## Problem statement

`--write-fix` decides comment placement (trailing end-of-line vs. leading
own-line vs. JSX `{/* */}`) purely from the *current* source text — see
`plan_file` in `src/fixer.rs`. It picks **trailing** whenever the violation
line's own end is lexically code and the resulting line fits under
`MAX_TRAILING_WIDTH` (100 chars). It has no model of what `biome check
--write` (a separate, later process almost every consumer runs right after,
since this tool's own suppression syntax needs Biome's formatter to still
produce readable code) will do to that line.

Biome's formatter attaches comments as token trivia during AST printing, not
by preserving raw line position. For a **single-line statement**, "trailing
comment at end of the line" and "trailing trivia on that statement's
semicolon token" are the same thing, so the comment survives reformatting.
But when the violation line is only the *first* physical line of a
multi-line construct — an arrow function whose body reformats, an object
literal whose properties get re-wrapped, a chained call — Biome may reprint
that token's trivia attached to a *different* printed line than the one
`--write-fix` saw. The comment is still in the file, still syntactically a
comment, but no longer on (or adjacent to) the line it was meant to suppress.
Because `custom-biome-lint`'s own suppression matching requires exact
line-adjacency (`find_suppression_comments` in `src/suppress/mod.rs` computes
`target_line` as literally `comment_line` or `comment_line + 1`), a
one-line drift is a complete, silent loss of coverage — the rule fires again
with no indication anything is wrong at the location that actually needs
attention.

`--write-fix` does self-verify (`verify()` in `src/fixer.rs`, called at the
end of `plan_file`) — but only against the source it just produced, before
any external formatter has touched it. That verification proves "this
suppression is correct right now," not "this suppression survives the next
tool in this rule's own recommended workflow."

## Concrete examples

All from the same commit
(`hornblower/UI/dashboard`, `36a7ade3ff4a9e32c5e0d6e4342abfc5c37b31a9`,
message `feat: enable bare-arrow-param-prop-assign and deep-param-prop-assign`)
and the manual fixups applied on top of it during that rollout. "Broken"
below is the state after `--write-fix` followed by `biome check --write` —
never committed, reconstructed here from the working session that found it.

### Baseline (safe): single-line statement, trailing form survives

No bug here — included for contrast, since it's the common case and shows
trailing placement is not wrong in general, only wrong for multi-line
targets.

```js
// src/components/InteractiveDashboard/InteractiveDashboard.jsx — final, stable
toolbar.getTabs = function () {
  const exportTab = tabs.find(({ title }) => title === 'Export') || {};
  (exportTab.menu || []).forEach(option => {
    option.handler = onExportHandler; // custom-biome-ignore-line bare-arrow-param-prop-assign
  });
};
```

`option.handler = onExportHandler;` is one line start to finish. Biome's
formatter has nothing to reflow, so the trailing comment's trivia stays
attached to the same printed line. Stable across repeated `biome check
--write` runs.

### Case 1 — multi-line arrow function body (the dominant failure shape)

`--write-fix` targets the violation line — the arrow's opening line,
`toolbar.getTabs = () => {` — for `bare-arrow-param-prop-assign` (the
mutation is the *assignment to `toolbar.getTabs`*, which the rule reports at
the arrow's start). Since that line's line-end is code and fits under 100
chars, it chooses **trailing**. But the arrow's body spans multiple lines, so
the comment's trivia gets attached to the printed *first statement inside the
block* by Biome's formatter, not to the opening line.

```js
// Before --write-fix (violation line: the `toolbar.getTabs = ...` line)
beforeToolbarCreated = toolbar => {
  const tabs = toolbar.getTabs();
  toolbar.getTabs = () => {
    const exportTab = tabs.find(tab => tab.id === 'fm-tab-export');
    ...
```

```js
// Immediately after --write-fix — looks fine, verify() passes against this exact text
beforeToolbarCreated = toolbar => {
  const tabs = toolbar.getTabs();
  toolbar.getTabs = () => { // custom-biome-ignore-line bare-arrow-param-prop-assign
    const exportTab = tabs.find(tab => tab.id === 'fm-tab-export');
    ...
```

```js
// BROKEN after `biome check --write`: comment relocated one line down,
// now a LEADING comment on `const exportTab`, no longer covering the
// mutation line at all. custom-biome-lint re-run reports the violation again.
beforeToolbarCreated = toolbar => {
  const tabs = toolbar.getTabs();
  toolbar.getTabs = () => {
    // custom-biome-ignore-line bare-arrow-param-prop-assign
    const exportTab = tabs.find(tab => tab.id === 'fm-tab-export');
    ...
```

```js
// Correct fix applied by hand: leading own-line form, placed above the
// violation line instead of trailing on it. Immune to the arrow body's
// own reformatting because it isn't attached to a token inside the arrow.
beforeToolbarCreated = toolbar => {
  const tabs = toolbar.getTabs();
  // custom-biome-ignore-next-line bare-arrow-param-prop-assign
  toolbar.getTabs = () => {
    const exportTab = tabs.find(tab => tab.id === 'fm-tab-export');
    ...
```

Same shape, three more occurrences in the same rollout:
`src/components/DailyFlashReport/DailyFlashReport.jsx` (×2, inside JSX
`beforeToolbarCreated={toolbar => { ... }}` props — see Case 3),
`src/components/InteractiveDashboard/InteractiveDashboard.jsx:195`,
`src/components/ReportViewer/ReportViewer.jsx:221` (both the
`toolbar.getTabs = function () { ... }` shape, non-arrow but same multi-line
body problem).

### Case 2 — object literal property, multi-line assignment target

```js
// src/selectors/eventBuilder.js — violation line is `accum[routeId][eventId] = {`
if (!(accum[routeId] || {}).hasOwnProperty(eventId)) {
  accum[routeId][eventId] = {
    eventId,
    published,
    ...
```

`--write-fix` chose trailing again (line fits, ends in `{` which is code).
After `biome check --write` reformatted the object literal, the comment's
trivia ended up attached to the first property instead:

```js
// BROKEN after format pass
if (!(accum[routeId] || {}).hasOwnProperty(eventId)) {
  accum[routeId][eventId] = {
    // custom-biome-ignore-line deep-param-prop-assign
    eventId,
    published,
```

```js
// Correct fix
if (!(accum[routeId] || {}).hasOwnProperty(eventId)) {
  // custom-biome-ignore-next-line deep-param-prop-assign
  accum[routeId][eventId] = {
    eventId,
    published,
```

### Case 3 — JSX children context, wrong marker form emitted

Per `src/fixer.rs`'s own doc comment, `--write-fix` is supposed to emit the
`{/* ... */}` form "when the insertion point falls in a JSX child list."  In
this rollout it did so for a location that is *not actually JSX children* —
the body of a plain-JS arrow function passed as a JSX **attribute value**
(`beforeToolbarCreated={toolbar => { ... }}`). The arrow's block body is
ordinary JS, where a normal `//` comment is completely valid; there is no
"comment becomes rendered text" hazard there. The `{/* */}` form is
needlessly used, and — combined with Case 1's line-drift — produces an inert
placement:

```js
// src/components/DailyFlashReport/DailyFlashReport.jsx — after --write-fix
beforeToolbarCreated={toolbar => {
  const tabs = toolbar.getTabs();
  {
    /* custom-biome-ignore-next-line bare-arrow-param-prop-assign */
  }
  toolbar.getTabs = () => tabs.slice(3);
}}
```

This is doubly wrong: `{ /* ... */ }` inside a plain JS function body is not
a comment at all — it's a **block statement containing an expression
statement that is a comment**, i.e. valid-but-inert JS, not a suppression
`custom-biome-lint` will ever recognize regardless of what line it targets.
`custom-biome-lint` re-run after this still reports the violation, with no
error pointing at the malformed suppression.

```js
// Correct fix: plain leading `//` comment, no braces, since this is JS
// code position, not a JSX child list.
beforeToolbarCreated={toolbar => {
  const tabs = toolbar.getTabs();
  // custom-biome-ignore-next-line bare-arrow-param-prop-assign
  toolbar.getTabs = () => tabs.slice(3);
}}
```

Same shape at `src/components/InteractiveDashboard/InteractiveDashboard.jsx`
(a `beforeToolbarCreated` prop) and twice in
`src/components/ManageOnlineShop/OnlineStoreOrdersTable.jsx` (an `onChange`
prop's arrow body):

```js
// BROKEN
<input
  type="date"
  value={filterList[index][0] || ''}
  onChange={event => {
    {
      /* custom-biome-ignore-next-line deep-param-prop-assign */
    }
    filterList[index][0] = event.target.value;
    onChange(filterList[index], index, column);
  }}
/>
```

```js
// Correct fix
<input
  type="date"
  value={filterList[index][0] || ''}
  onChange={event => {
    // custom-biome-ignore-next-line deep-param-prop-assign
    filterList[index][0] = event.target.value;
    onChange(filterList[index], index, column);
  }}
/>
```

#### Root cause (confirmed against source — not an inference)

`src/fixer.rs`, `JsxText::collect` and `JsxText::contains`:

```rust
struct JsxText {
    child_lists: Vec<(usize, usize)>,
    expressions: Vec<(usize, usize)>,
}

impl JsxText {
    fn collect(tree: &JsSyntaxNode) -> Self {
        let mut child_lists = Vec::new();
        let mut expressions = Vec::new();

        for node in tree.descendants() {
            match node.kind() {
                JsSyntaxKind::JSX_CHILD_LIST => child_lists.push(span(node.text_range())),
                JsSyntaxKind::JSX_EXPRESSION_CHILD => {
                    expressions.push(span(node.text_trimmed_range()))
                }
                _ => {}
            }
        }

        Self { child_lists, expressions }
    }

    fn contains(&self, offset: usize) -> bool {
        let in_children = self
            .child_lists
            .iter()
            .any(|&(start, end)| start <= offset && offset <= end);
        if !in_children {
            return false;
        }
        // Inside `{ ... }` we are back in ordinary expression context.
        !self
            .expressions
            .iter()
            .any(|&(start, end)| start < offset && offset < end)
    }
}
```

`contains()` is a pure **byte-offset range check** against `child_lists`,
not a tree-ancestry check. A `JSX_CHILD_LIST` node's `text_range()` spans
from the start of its first child to the end of its last child — and when a
child is itself a JSX element with attributes (`<PivotTable
beforeToolbarCreated={toolbar => { ... }} ... />` as a child of `<Paper>`),
every byte of that element — including deep inside its attribute values —
falls textually within the parent child list's `(start, end)` span. The
`expressions` carve-out only excludes byte ranges wrapped in a
`JSX_EXPRESSION_CHILD` (an explicit `{expr}` used *as a child*, e.g.
`<div>{cond && <Foo/>}</div>`). A JSX element passed directly as a child with
no wrapping `{}` — the ordinary case, `<PivotTable ... />` as a bare child of
`<Paper>` — produces no `JSX_EXPRESSION_CHILD` node at all, so nothing carves
its attribute values back out. The result: any offset inside
`beforeToolbarCreated={toolbar => { ... }}`'s arrow body is reported as JSX
child text purely because that attribute physically sits inside the
surrounding `<Paper>...</Paper>` child list's byte span, even though the
attribute value is ordinary JS, several tree-levels away from being a
`JSX_CHILD_LIST` member itself.

This is a distinct, independently-triggerable bug from Case 1/2's
line-relocation problem — it fires even for a single-line arrow body, and
would fire even if the primary fix (statement-boundedness) were already
implemented, since it corrupts the marker's *form*, not its *placement line*.

#### Proposed fix (JSX detection)

Replace the pure byte-range containment check with an actual ancestry check:
walk up from the insertion point's node to determine whether its nearest
JSX-relevant ancestor is a `JSX_CHILD_LIST` entry itself (this insertion
point is genuinely rendered JSX text/an expression child) versus a
`JsxAttributeInitializerClause` / `JsxExpressionAttributeValue` (this
insertion point is JS reached only *through* an attribute, and should be
treated as plain code no matter how deeply the surrounding tree is nested in
JSX). Concretely: instead of recording `(start, end)` spans and doing a
numeric interval check, record whether each candidate insertion offset's
*direct parent chain* passes through a `JSX_CHILD_LIST` before it passes
through any `JsxAttribute`-family node — if an attribute node is hit first,
the offset is not JSX children, full stop, regardless of any enclosing child
list further up. This requires walking from the insertion point upward
(`JsSyntaxNode::ancestors()`), not scanning `tree.descendants()` and
recording flat spans — the current approach — since only ancestry reflects
"which JSX construct does this point actually belong to," not "which JSX
construct's byte range happens to overlap it."

### Case 4 — circular adjacency conflict with a pre-existing `biome-ignore`

Not a formatter-relocation bug, but the same root problem (adjacency is
assumed stable, isn't checked against a second suppression system) and it
surfaced in the same rollout, on the same line, so it belongs in this report.

`src/selectors/billingDashboard.js` already carried a ported
`biome-ignore lint/style/noParameterAssign` comment (from an earlier,
separate migration) directly above the target line. `--write-fix` inserted
its own leading comment **between** that `biome-ignore` and the code:

```js
// After --write-fix: three suppression-adjacent comments stacked
if (!accum.stackedData[valKey]) {
  // eslint-disable-next-line no-param-reassign
  // biome-ignore lint/style/noParameterAssign: ported from prior eslint-disable (no-param-reassign)
  // custom-biome-ignore-next-line deep-param-prop-assign
  accum.stackedData[valKey] = {};
```

Biome's own suppression comments must be the *immediate* leading trivia of
the token they cover — a comment sitting between `biome-ignore` and the code
breaks Biome's own comment, which it reports explicitly
(`suppressions/unused`: "Suppression comment has no effect"). Reordering to
put `custom-biome-ignore-next-line` first does not help — swap the two and
`custom-biome-lint`'s *own* adjacency check (`target_line = comment_line + 1`
in `src/suppress/mod.rs`) is what breaks instead, since now a `biome-ignore`
comment sits between the marker and the code. **Neither tool's leading-form
suppression syntax allows another unrelated comment between the marker and
the code it targets** — so two independently-authored leading suppressions
can never both point at the same one-line target.

The only placement that satisfies both tools at once here is put one of them
in **trailing** form (safe in this specific case because the target
statement, `accum.stackedData[valKey] = {};`, is single-line):

```js
// Correct fix — biome-ignore stays leading+adjacent (Biome's requirement);
// custom-biome-ignore-line moves to trailing on the code line itself.
if (!accum.stackedData[valKey]) {
  // eslint-disable-next-line no-param-reassign
  // biome-ignore lint/style/noParameterAssign: ported from prior eslint-disable (no-param-reassign)
  accum.stackedData[valKey] = {}; // custom-biome-ignore-line deep-param-prop-assign
```

`--write-fix` has no awareness that a `biome-ignore` (not one of its own
markers) already occupies the immediately-preceding line, so it can't know
to prefer trailing here on its own.

#### Root cause

Two independent, narrow contracts collide; neither is wrong in isolation.

`src/suppress/mod.rs`, `find_suppression_comments` — this tool's own marker
is defined as targeting only the physically adjacent line:

```rust
let (marker, target_line, marker_pos) = if let Some(pos) = comment.find(IGNORE_NEXT_LINE) {
    (IGNORE_NEXT_LINE, line_no + 1, pos)
} else if let Some(pos) = comment.find(IGNORE_LINE) {
    (IGNORE_LINE, line_no, pos)
} else {
    continue;
};
```

`target_line` is always exactly `line_no` (same line) or `line_no + 1` (the
literal next physical line) — there is no window, no "skip past other
comment lines" logic. Biome's own `biome-ignore` marker has the identical
contract in the opposite direction: it is documented (and enforced, per the
`suppressions/unused` diagnostic seen in this rollout) to apply only to the
token immediately following it, with no comment permitted in between.

`plan_file` in `src/fixer.rs` never reads `find_suppression_comments`'
output for the *specific purpose* of checking "is there a foreign
(non-`custom-biome-ignore`) suppression-shaped comment sitting on the line
directly above my intended own-line insertion point?" It does call
`find_suppression_comments` and build `by_target`/`marked_lines`
(see `plan_file`'s setup, `let existing = find_suppression_comments(source);`
near its start) — but only to decide whether *this tool's own* marker
already covers the line (the `by_target.get(&line)` merge-path). A
`biome-ignore` comment is invisible to that lookup because
`find_suppression_comments` only recognizes `custom-biome-ignore-line` /
`custom-biome-ignore-next-line`, by design (it's this tool's own suppression
parser, not Biome's) — so `plan_file` has no signal at all that inserting
`OwnLine` one line above the target would land directly on top of, or
directly below, a `biome-ignore` that has the same "must be immediately
adjacent, nothing in between" requirement.

#### Proposed fix

Give `plan_file` a second, narrower scan — independent of
`find_suppression_comments`, since that function intentionally only
recognizes this tool's own markers — that checks the raw line immediately
above the violation line for *any* comment matching a generic
suppression-comment shape (`biome-ignore`, `eslint-disable(-next-line)?`, or
this tool's own markers already handled via `by_target`). A simple regex
(`^\s*//\s*(biome-ignore|eslint-disable)\b`) is enough; this does not need
to understand the foreign tool's suppression semantics, only recognize
"something here has claimed adjacency already." When that scan finds a hit
and the target statement is single-line (per the primary fix's
statement-boundedness check), prefer `Trailing` over `OwnLine` specifically
to leave the foreign comment's adjacency undisturbed. When the target
statement is *not* single-line, there is no placement that satisfies both
tools simultaneously — report it as a new `Unfixable` reason (e.g.
`"leading line already claimed by another tool's suppression comment, and
target spans multiple lines"`) rather than writing the three-comments-stacked
result seen in this rollout, which silently breaks the pre-existing
`biome-ignore` with no diagnostic pointing at the cause.

## Root cause

`src/fixer.rs`, `plan_file()` (roughly lines 195–260 as of this writing) and
its `trailing_ok` computation:

```rust
let trailing_ok = matches!(end_state, Lex::Code | Lex::LineComment)
    && !marked_lines.contains(&line)
    && !jsx.contains(line_start + content.len());
if trailing_ok {
    let comment = comment_text(IGNORE_LINE, &rules, false);
    if content.chars().count() + 1 + comment.chars().count() <= MAX_TRAILING_WIDTH {
        appends.insert(line, format!(" {comment}"));
        ...
```

Three independent gaps, all in this function:

1. **No "is this line the whole statement" check.** `trailing_ok` only asks
   whether the *lexical* end of the physical line is code (not inside a
   string/comment) and whether the resulting line length fits. It never asks
   whether `line` is the last physical line of the enclosing statement/
   expression. A physical line ending in `{`, `(`, or a property name
   followed by `,` is exactly as "trailing_ok" as one ending in `;` — but
   only the latter is safe against reformatting, because only there does the
   line's last token and the statement's last token coincide.
2. **`JsxText::contains` is a byte-range containment check, not an ancestry
   check — confirmed against source.** `JsxText::collect` (`src/fixer.rs`,
   below `plan_file`) records the flat `(start, end)` byte span of every
   `JSX_CHILD_LIST` node in the tree, then carves back out only the spans
   covered by a `JSX_EXPRESSION_CHILD` (an explicit `{expr}` used as a
   child). A JSX element passed directly as a child with no wrapping braces
   — the ordinary case, e.g. `<PivotTable beforeToolbarCreated={toolbar =>
   {...}} />` as a bare child of `<Paper>` — produces no
   `JSX_EXPRESSION_CHILD` node, so nothing carves its attribute values back
   out of the parent child list's span. Since every byte of that nested
   element, including deep inside its attribute values, falls textually
   within the parent `JSX_CHILD_LIST`'s `(start, end)` range, `contains()`
   reports "yes, this is JSX child text" for offsets that are actually
   ordinary JS reached only through an attribute. See Case 3's dedicated
   "Root cause (confirmed against source)" subsection above for the full
   quoted implementation and the fix this implies (an ancestry walk instead
   of a flat interval check).
3. **`verify()` only checks the tool's own output, not the downstream
   formatter's.** `verify()` (`src/fixer.rs`) re-parses `rewritten` and
   re-runs `Suppressions::parse` against it — a real and valuable check, but
   it validates against the exact bytes `--write-fix` itself produced. It
   has no step that asks "if the user's own `biome check --write` (or
   `biome format --write`) runs next, does the suppression still land on the
   right line?" Since this tool's suppression syntax is specifically
   documented to coexist with Biome (`biome-ignore` ports exist right next
   to `custom-biome-ignore` markers throughout the dashboard codebase), and
   every consumer's real workflow is "run `--write-fix`, then run Biome's
   formatter," that combined sequence is the thing that actually needs to
   stay verified — not `--write-fix`'s output in isolation.

Separately, `find_suppression_comments` (`src/suppress/mod.rs`) computes
`target_line` as a strict `comment_line ± 1` with **no tolerance for another
comment line sitting in between**. This is not itself wrong — it's a
reasonable, simple contract — but it's the reason Case 4 has no placement
that works with a leading form on both sides. Any fix for Cases 1–3 should
keep this contract in mind: prefer leading own-line placement (this report's
recommendation below) only helps until two independent tools both want the
adjacent leading line for the same target.

## Proposed fix

**Primary recommendation: change `--write-fix`'s default policy for
non-single-line targets from "trailing when it fits" to "leading own-line
unless the enclosing statement is provably single-line."**

Concretely: `trailing_ok` should require not just that the physical line's
lexical end is code, but that the line's last non-trivial token is also the
*last token of the smallest enclosing statement* covering the violation
line — i.e., the statement starts and ends within the reported line. This is
answerable from the parsed tree `plan_file` already holds
(`FileContext::parse`): walk up from the violation's node to its nearest
`JsStatement`-ish ancestor and compare `range().start().line` /
`range().end().line` (via the existing `line_offsets`/`starts` machinery) —
if they differ, force `OwnLine`.

This is a **blanket policy change**, not smarter per-shape detection,
because:

- It's simple to state, simple to verify, and matches the one dimension that
  actually determines survivability: statement-boundedness, not line length
  or lexical category. A smarter heuristic ("safe unless there's a `{` or
  `(` immediately before the comment") would need to special-case every new
  JS/JSX construct Biome's formatter might reflow differently in the future
  (template literals, JSX attribute lists, arrow chaining, decorators…);
  "is the statement single-line" already generalizes over all of them.
- The dashboard rollout this bug came from produced correct results for
  *zero* multi-line targets using trailing form — every one of the 9 breaks
  was a multi-line target. There is no evidence in this corpus that trailing
  placement is worth the risk for anything but single-line statements.
- It keeps the tool's suppression style closer to what a human already does
  by convention in this codebase (see the fixtures under `fixtures/*/`,
  which are essentially all leading-form already), reducing the diff between
  `--write-fix` output and what a reviewer would hand-write.

**Secondary recommendation, worth doing regardless: fix the JSX-context
detection (root cause #2)** so `comment_text()`'s `jsx` flag is keyed
specifically on "is this insertion point inside a `JsxChildList`," not
merely "is this node a descendant of some JSX element." A function passed as
a prop value is JS, not JSX children, no matter how deeply the JSX
surrounding it is nested.

**Tertiary recommendation, larger scope — self-verify against Biome's
formatter, not just the parser:** `verify()` already re-parses; it should
additionally invoke Biome's own formatter on `rewritten` (this tool already
depends on `biome_js_syntax`; the sibling formatter crate is a natural
addition, or shelling out to a `biome` binary already on `PATH` if pulling
the crate in is too heavy) and re-run `Suppressions::parse` + line-mapping
against *that* output too, not only the pre-format text. If the two runs
disagree about whether every change is still suppressed, treat that as a
verify failure exactly like a parse failure today — fall back to
`Unfixable` with a distinct reason string
(`"formatter would relocate suppression"`), so the user gets an honest
"could not auto-fix, do this by hand" for the ~4% of cases (9/218 measured
here) instead of a silent gap discoverable only by re-linting after
formatting.

This third recommendation is the most robust fix (it makes correctness
independent of correctly predicting every way Biome's printer may reflow
trivia, present or future) but has real cost: pulling in or shelling out to
Biome's formatter from this tool's own fix path, and running it per-file on
every `--write-fix` invocation. The primary recommendation (statement-
boundedness check) fixes 100% of the cases actually observed with no new
dependency, so it should land first; the formatter-based self-verify is a
good follow-up hardening step, not a blocker.

Case 4 (circular adjacency) isn't fixed by any of the above — it needs its
own, narrower fix: when planning placement, `plan_file` should already scan
`by_target`/`marked_lines` (from `find_suppression_comments`) for a
**foreign** comment (a `biome-ignore` or anything not one of this tool's own
markers) that occupies the line immediately above the violation line, and if
one exists and there is no already-established `custom-biome-ignore` there
too, prefer `Trailing` (when the target statement is single-line, per the
primary fix above) over `OwnLine` specifically to avoid displacing that
foreign comment's adjacency to the code. If the target statement is *not*
single-line in that situation, this becomes a genuinely unfixable case
(no placement satisfies both tools) and should be reported as `Unfixable`
with reason `"leading line already occupied by another tool's suppression
comment and target spans multiple lines"` rather than silently producing a
broken result.

## Test cases

Fixture-style, matching the shape used under `fixtures/*/`
(`valid.js` / `invalid.js` / `suppressed.js` / `edge-cases.js`). These are
new **fixer** tests, not rule tests — they belong in a new
`fixtures/write_fix_placement/` directory (or wherever `--write-fix`-specific
fixtures should live; check whether `tests/integration.rs` already has a
fixer-focused test module before creating one) and should assert both (a)
what `--write-fix` writes immediately, and (b) that running the equivalent
of `biome check --write` over the result and re-parsing for suppressions
still finds every violation covered. (b) is the part that would have caught
this bug; a fixture that only checks (a) would have passed throughout this
whole incident.

| # | Shape | Before | `--write-fix` today (placement) | After a real `biome format` pass | Should be (with the fix) |
|---|---|---|---|---|---|
| 1 | Single-line statement (baseline, must stay unchanged) | `option.handler = onExportHandler;` | trailing, same line | unchanged | trailing (no regression) |
| 2 | Multi-line arrow body, violation on arrow's opening line | `toolbar.getTabs = () => {\n  const exportTab = ...` | trailing, on `{` line | comment relocates to leading-on-`const exportTab` | leading own-line, above `toolbar.getTabs = () => {` |
| 3 | Multi-line arrow assigned via `function ()` (non-arrow, same body-reflow risk) | `toolbar.getTabs = function () {\n  const exportTab = ...` | trailing, on `{` line | relocates same as #2 | leading own-line |
| 4 | Multi-line object literal property target | `accum[routeId][eventId] = {\n  eventId,\n  published,` | trailing, on `{` line | relocates to leading-on-`eventId,` | leading own-line, above `accum[routeId][eventId] = {` |
| 5 | JSX-attribute arrow body (plain JS, not JSX children) | `onChange={event => {\n  filterList[index][0] = event.target.value;` | `{/* */}` own-line form (wrong marker choice) | inert regardless of formatting — never a real comment | plain `//` leading own-line (no braces) |
| 6 | True JSX children context (contrast case — confirm this one is still handled correctly) | `<div>{cond && <Foo onClick={x => { x.y = 1; }} />}</div>` inside actual child position | `{/* */}` form | should remain stable, needs a regression fixture confirming Biome doesn't reflow it either | `{/* */}` form (no change — just needs a test proving it) |
| 7 | Circular adjacency: foreign leading `biome-ignore` already directly above a single-line target | `// biome-ignore lint/style/noParameterAssign: ...\naccum.x = {};` | today: own-line inserted between, breaking the `biome-ignore` | Biome reports "Suppression comment has no effect" on the now-non-adjacent `biome-ignore` | trailing custom-biome-ignore-line on the code line, `biome-ignore` untouched and still adjacent |
| 8 | Circular adjacency, but target is multi-line (no safe placement exists) | `// biome-ignore lint/style/noParameterAssign: ...\naccum.x = {\n  y: 1,\n};` | today: same own-line insertion, same breakage | same breakage | reported as `Unfixable`, not silently written |
| 9 | Chained/nested call expression spanning multiple lines (mentioned in the task as a shape to cover — verify whether it independently reproduces the bug or is subsumed by #2/#4) | `accum[diffInDays].orderTotal =\n  accum[diffInDays].orderTotal + summary.total + ...;` (real shape from `src/api/hb/graphql/index.js` in the same rollout) | trailing, on the `accum[diffInDays].orderTotal = // comment` line — this one happened to survive in the actual rollout since the comment attaches to the assignment's LHS token, which itself doesn't move | needs a regression fixture either way — confirm whether this is coincidentally safe or fragile | leading own-line if the statement-boundedness check says so; add the fixture regardless since it's a real corpus shape that wasn't manually caught as broken (may have gotten lucky) |

## TODO / implementation checklist

Written so a fresh session with no other context can pick this up.

1. **Confirm root cause #2 (JSX detection) directly against source.** Read
   `JsxText::collect` in full (`src/fixer.rs`, below `plan_file`) and
   determine exactly what node kind its `child_lists`/`expressions` ranges
   are computed from. Confirm (with a minimal repro file) whether a
   plain-JS arrow function nested inside a JSX attribute value is currently
   included in those ranges. Write down the actual condition before changing
   it — the description in this doc is inferred from *symptoms*, not yet
   read line-by-line against current `JsxText::collect` code.
2. **Add the statement-boundedness check.** In `plan_file`, before computing
   `trailing_ok`, walk from the violation's target token up to its nearest
   enclosing statement-like ancestor (reuse whatever `file.semantic()` /
   `context.tree()` traversal helpers already exist — check
   `src/rules/param_mutation.rs` if the shared helper module from
   `DESTRUCTURED_PARAM_MUTATION_RULES_PLAN.md` step 2 has landed, it likely
   already has ancestor-walking utilities worth reusing rather than
   duplicating). Compute the ancestor's start/end line via the same
   `line_offsets`/`starts` arrays `plan_file` already builds. Require
   `start_line == end_line == violation_line` as an additional condition
   inside `trailing_ok`, alongside the existing lexical/width checks.
3. **Fix root cause #2** once confirmed in step 1: narrow `comment_text()`'s
   `jsx` decision to specifically "insertion point is inside a
   `JsxChildList`," not "insertion point is anywhere under a JSX-bearing
   subtree."
4. **Add the Case 4 (circular adjacency) detection.** In `plan_file`, when
   choosing between `Trailing` and `OwnLine`, check whether the line
   immediately above the violation line is already a suppression-shaped
   comment (`biome-ignore`, or any comment matching a
   suppression-comment-like pattern) that is *not* one of this tool's own
   markers on this same target. If so and the statement is single-line,
   prefer `Trailing`; if the statement is not single-line, add the new
   `Unfixable` reason from the Proposed Fix section instead of writing a
   broken result.
5. **Add the fixture directory and cases from the Test Cases table above.**
   Each row becomes at minimum a "before" fixture file and an assertion
   both immediately after `--write-fix` and after simulating (or, if
   feasible, actually invoking) a Biome format pass over the result. If this
   repo doesn't already have infrastructure to invoke Biome's formatter
   in-process or via a bundled binary during tests, that's a prerequisite
   sub-task — check `Cargo.toml` for an existing `biome_formatter`/
   `biome_js_formatter` dependency or similar before assuming one needs to
   be added.
6. **Add unit tests for `verify()`'s new formatter-aware mode** (tertiary
   recommendation), gated so the primary fix (statement-boundedness) is not
   blocked on this landing — this can be a separate, later PR.
7. **Update `docs/RULES.md` and/or `src/cli/output.rs`'s `--help` text** if
   the default placement policy changes user-visible behavior enough to
   warrant a note (e.g., "own-line is now preferred for any multi-line
   target; trailing is reserved for statements that begin and end on the
   violation line").
8. **Integration sanity check against the real corpus that found this bug.**
   Re-run this fixed `--write-fix` against a fresh checkout of
   `hornblower/UI/dashboard` at the commit *before*
   `36a7ade3ff4a9e32c5e0d6e4342abfc5c37b31a9` (i.e. `042db0bc0`, the parent
   commit) with `bare-arrow-param-prop-assign` and `deep-param-prop-assign`
   turned on via `ignoreBiomeExtensionRules`, exactly like that rollout did.
   Then run `biome check --write src cypress` over the result and re-run
   `custom-biome-lint`. Expect **0 violations after the format pass**,
   without any manual comment-form correction — that's the regression test
   this whole bug report exists to satisfy. (This is read-only against that
   repo — do not commit anything there as part of this check; it's a
   verification step, not a deliverable.)
9. **Run the existing suite (`cargo test && cargo clippy --all-targets`)**
   before considering this done, per the convention in
   `DESTRUCTURED_PARAM_MUTATION_RULES_PLAN.md`'s own checklist.

## Non-goals for this plan

- Not attempting to make `--write-fix` aware of arbitrary third-party tools
  other than Biome's formatter specifically (e.g. Prettier) — this tool's
  suppression syntax and its whole raison d'être is coexistence with Biome,
  not with every possible formatter a consumer might run.
- Not proposing to remove trailing placement entirely. It's correct and
  desirable for the overwhelmingly common single-line-statement case (see
  the fixtures under `fixtures/*/suppressed.js`, which already lean on
  trailing form for exactly that shape) and produces less visual noise than
  always forcing own-line.
- Not attempting to fully replicate Biome's comment-attachment algorithm
  in-process for the primary fix — the statement-boundedness heuristic is
  deliberately conservative (some genuinely-safe trailing placements on
  multi-line-but-comment-stable constructs may get pushed to own-line
  unnecessarily) rather than exactly modeling Biome's printer. The tertiary
  recommendation (format-and-reverify) is where exactness would actually
  live, if ever implemented.
