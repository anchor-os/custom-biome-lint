// Plain parameter — Biome's noParameterAssign with propertyAssignment: "deny"
// already flags this at depth 1.
export function plainPropAssign(d) {
  d.token = 'x';
}

// Reading a property is not writing one.
export function readProperty({ c }) {
  return c.token;
}

// Reassigning the binding itself is destructure-default-param-assign's job, so
// this rule stays quiet and the two never both report one line.
export function reassignment({ c }) {
  // custom-biome-ignore-next-line destructure-default-param-assign -- the near-miss is the point
  c = null;
}

// Known non-goal: aliasing. `local`'s own binding is a `const`, not a
// parameter, so the mutation resolves out of this rule's scope. Tracking it
// would need real dataflow analysis; see docs/RULES.md.
export function aliased({ payload }) {
  const local = payload;
  local.token = 'x';
}

// Known non-goal: mutating method calls. There is no assignment node here at
// all — the same boundary noParameterAssign itself draws.
export function methodCall({ payload }) {
  payload.items.push(1);
  payload.items.splice(0, 1);
}

// A local shadowing the parameter resolves to the local.
export function shadowed({ payload }) {
  {
    const payload = {};
    payload.token = 'x';
  }
}

// Copying before mutating — the fix this rule's message asks for.
export function copiedFirst({ payload }) {
  const next = { ...payload };
  next.token = 'x';
  return next;
}
