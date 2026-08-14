// Plain parameter — Biome's noParameterAssign already flags this, so leaving
// it alone is what keeps the two tools from double-reporting one line.
export function plainParam(a) {
  a = 5;
  return a;
}

// Reading a destructured parameter is not reassigning it.
export function readOnly({ b = '' }) {
  return b.trim();
}

// A property write, not a reassignment of the binding — that is
// destructure-param-prop-assign's territory, not this rule's.
export function propertyWrite({ c }) {
  // custom-biome-ignore-next-line destructure-param-prop-assign -- the near-miss is the point
  c.token = 'x';
}

// A separate local variable that merely holds the parameter's value. Its own
// binding is a `let`, not a parameter.
export function localCopy({ b = '' }) {
  let local = b;
  local = 'x';
  return local;
}

// A local declared inside the function shadows the destructured parameter, so
// this assignment resolves to the local, not the parameter.
export function shadowed({ value }) {
  {
    let value = 1;
    value = 2;
    return value;
  }
}

// Destructuring a plain (non-destructured) parameter inside the body creates
// ordinary locals, not parameter bindings.
export function destructuredLocal(action) {
  let { payload } = action;
  payload = null;
  return payload;
}
