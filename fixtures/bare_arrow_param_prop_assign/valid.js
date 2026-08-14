// Parenthesized single parameter — Biome's noParameterAssign already flags
// this, which is the whole reason the bare form needs its own rule.
export const parenthesised = (d) => {
  d.token = 'x';
};

// Multi-parameter arrows are always parenthesized by the JS grammar, so they
// are never the missed shape.
export const multiParam = (accum, item) => {
  accum.token = item;
};

// Named function declarations and expressions are a different AST shape and
// already covered.
export function named(d) {
  d.token = 'x';
}

export const functionExpression = function (d) {
  d.token = 'x';
};

// A destructured single parameter must be parenthesized, and is
// destructure-param-prop-assign's territory anyway — which that always-on rule
// duly reports here, hence the marker naming it and not this rule.
export const destructured = ({ c }) => {
  // custom-biome-ignore-next-line destructure-param-prop-assign -- the near-miss is the point
  c.token = 'x';
};

// Reading a property, not writing one.
export const read = item => item.x;

// Reassigning the bare parameter itself. Biome *does* flag this — verified
// against 2.5.8 — so this rule deliberately stays out of it rather than
// double-reporting.
export const reassign = item => {
  item = null;
  return item;
};

// A local shadowing the bare parameter resolves to the local.
export const shadowed = item => {
  {
    const item = {};
    item.x = 1;
  }
};
