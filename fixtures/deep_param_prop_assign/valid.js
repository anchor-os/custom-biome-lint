// Depth 1 — exactly what Biome's noParameterAssign already reports with
// propertyAssignment: "deny". Re-reporting it here under a second rule name
// would be noise, so the depth floor is deliberate.
export function depthOneBracket(acc, x) {
  acc[x] = 1;
}

export function depthOneDot(d) {
  d.token = 'x';
}

// Destructured root — destructure-param-prop-assign's territory. This rule's
// eligibility is the inverse, so a destructured root never double-fires here.
export function destructuredRoot({ acc }, x, y) {
  // custom-biome-ignore-next-line destructure-param-prop-assign -- the near-miss is the point
  acc[x][y] = 1;
}

// Reading a deep chain is not writing to it.
export function deepRead(state, id) {
  return state.tours[id].priceBands;
}

// A mutating method call has no assignment node at all — the same boundary
// noParameterAssign draws.
export function methodCall(state, id) {
  state.tours[id].priceBands.push(1);
}

// Not a parameter: a local, however deeply mutated.
export function localRoot() {
  const acc = {};
  acc.a.b = 1;
  return acc;
}

// A local shadowing the parameter resolves to the local.
export function shadowed(acc) {
  {
    const acc = {};
    acc.a.b = 1;
  }
}

// Reassigning the parameter itself, at no depth — Biome covers that.
export function reassignment(acc) {
  acc = {};
  return acc;
}
