// A default value only decides the binding's initial value, never whether it
// is a destructured parameter — these two must be reported identically.
export function withDefault({ b = '' }) {
  b = 'x';
}

export function withoutDefault({ b }) {
  b = 'x';
}

// Nesting depth is irrelevant: any destructuring pattern between the binding
// and the parameter list qualifies.
export function deeplyNested({ a: { b: { c } } }) {
  c = 1;
}

export function objectInsideArray({ items: [{ id }] }) {
  id = 2;
}

// Rest siblings: `rest` is itself a destructured binding, and `a`'s status is
// unaffected by the rest element sitting beside it. Both are reported.
export function restSibling({ a, ...rest }) {
  a = 1;
  rest = {};
}

export function arrayRest([head, ...tail]) {
  head = 1;
  tail = [];
}

// Compound assignment and the update operators are reassignment too, exactly as
// noParameterAssign treats them for plain parameters.
export function operators({ count = 0 }) {
  count += 1;
  count -= 1;
  count++;
  --count;
}

// A for-of/for-in head assigning into an existing binding is a reassignment.
export function loopHeads({ item }, list, object) {
  for (item of list) {
    void item;
  }
  for (item in object) {
    void item;
  }
}

// A destructuring *assignment* (no declaration keyword) writing back into the
// destructured parameter binding.
export function destructuringAssignment({ a, b }, pair, source) {
  [a] = pair;
  ({ b } = source);
}

// Shadowing resolves through the semantic model, not by name: the outer `token`
// here is a destructured parameter, but the assignment inside the inner
// function targets that function's own plain parameter and is left to Biome.
export function shadowing({ token }) {
  function inner(token) {
    token = 'x';
    return token;
  }
  return inner(token);
}

// Arrow functions have no bare-parameter variant of this rule to test: JS
// requires parentheses around a destructured parameter, so `{ x } => ...` is a
// syntax error and a bare single arrow parameter can never be destructured.
export const parenthesisedIsTheOnlyForm = ({ x }) => {
  x = 1;
};
