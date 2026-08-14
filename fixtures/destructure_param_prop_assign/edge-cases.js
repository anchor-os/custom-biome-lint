// Depth independence is the headline difference from Biome's own rule, which
// catches exactly one level and misses two or more. All three of these are
// reported identically.
export function depthOne({ c }) {
  c.token = 'x';
}

export function depthTwo({ acc }, k) {
  acc[k].total = 'x';
}

export function depthThree({ state }, id) {
  state.tours[id].priceBands = {};
}

// Mixed dot/bracket notation, both ways round — the root walk doesn't care
// which form each hop uses.
export function mixedNotation({ state }, id) {
  state['tours'][id].priceBands = {};
  state.tours['byId'][id] = {};
}

// Array destructuring roots the chain just as object destructuring does.
export function arrayDestructured([head]) {
  head.token = 'x';
}

// Rest siblings: `rest` is a destructured (rest) binding like any other.
export function restSibling({ a, ...rest }) {
  a.x = 1;
  rest.x = 1;
}

// Compound and update writes are property mutations too.
export function operators({ totals }, key) {
  totals[key] += 1;
  totals.count++;
  --totals.count;
}

// Nested destructuring: the binding is several patterns deep in the parameter
// list, and the mutation is several hops deep in the chain.
export function nestedBinding({ outer: { inner } }, id) {
  inner.items[id] = 1;
}

// A for-of head writing into a property of a destructured parameter.
export function loopHead({ target }, list) {
  for (target.current of list) {
    void target.current;
  }
}

// Not applicable: `payload?.token = 'x'` is not valid JS — optional chaining is
// not assignable — so there is no optional-chaining write case to cover.

// Depth-1 mutation of a plain parameter alongside a destructured one, in the
// same signature: only the destructured root is this rule's concern.
export function mixedParameters({ payload }, plain) {
  payload.token = 'x';
  plain.token = 'x';
}
