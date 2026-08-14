// Nested arrows: the mutation sits inside a *different* arrow's body than the
// one declaring the parameter. Resolution is by binding, not by lexical
// nesting, so `item` still attributes to the outer arrow's parameter.
export const nestedArrows = (arr, other) =>
  arr.map(item =>
    other.forEach(x => {
      item.y = 1;
      void x;
    }),
  );

// An expression-bodied arrow — no block, and the mutation is the body itself.
export const expressionBody = item => (item.x = 1);

// Chain depth is not this rule's axis: depth 1 and depth 2+ both report, and a
// depth-2 write on a bare parameter also trips deep-param-prop-assign. Both
// rules fire independently by design; neither suppresses the other.
export const depthOne = item => {
  item.a = 1;
};

export const depthTwo = item => {
  item.a.b = 1;
};

// Compound and update writes.
export const operators = row => {
  row.total += 1;
  row.count++;
};

// A bare parameter of an async arrow is the same shape.
export const asyncBare = async item => {
  item.x = 1;
};

// The bare parameter of an inner arrow, mutated in that inner arrow — the
// common `map(cb)` shape, reported against the inner parameter.
export const innerOnly = list => list.map(entry => (entry.seen = true));

// A rest parameter is written `(...rest) =>` and so is always parenthesized;
// there is no bare rest form to cover.
export const restIsAlwaysParenthesised = (...rest) => {
  rest[0].x = 1;
};
