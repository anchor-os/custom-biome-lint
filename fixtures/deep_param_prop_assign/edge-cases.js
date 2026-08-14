// The depth boundary, in one place: depth 1 is left to Biome, depth 2 and
// deeper are this rule's.
export function depthBoundary(acc, x, y, z) {
  acc[x] = 1;
  acc[x][y] = 1;
  acc[x][y][z] = 1;
}

// Arrow-parens style is irrelevant here — that is
// bare-arrow-param-prop-assign's axis. Both forms report identically.
export const parenthesised = (acc, x) => {
  acc.items[x] = 1;
};

export const bareSingleParam = acc => {
  acc.items.first = 1;
};

// Mixed dot/bracket chains, both orders.
export function mixedNotation(state, id) {
  state['tours'][id].priceBands = {};
  state.tours['byId'][id] = {};
}

// Compound and update writes at depth 2+.
export function operators(totals, key) {
  totals.byKey[key] += 1;
  totals.byKey.count++;
  --totals.byKey.count;
}

// A rest parameter is a plain parameter for this rule's purposes.
export function restParam(...groups) {
  groups[0].items = [];
}

// Parentheses inside the chain are transparent and do not count as a hop, so
// this stays a depth-1 write and is left to Biome.
export function parenthesisedChain(acc) {
  (acc).token = 'x';
}

// A for-of head writing two levels deep.
export function loopHead(target, list) {
  for (target.state.current of list) {
    void target.state.current;
  }
}

// A chain not rooted in a plain identifier at all (a call result, `this`) has no
// parameter to attribute the write to.
export function unrootedChain(make) {
  make().a.b = 1;
}
