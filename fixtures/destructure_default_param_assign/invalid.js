// Every reassignment below targets a binding introduced by destructuring in a
// parameter list, which Biome's noParameterAssign does not reach.

export function destructuredDefault({ b = '' }) {
  b = 'x';
}

export const withoutDefault = ({ x }) => {
  x = 1;
};

export function arrayDestructure([first]) {
  first = first.trim();
}

export function nested({ outer: { inner } }) {
  inner = 'x';
}

export const generateBarcodeSuggestions = ({ prefix = '' }) => {
  if (prefix.length < 8) {
    return prefix;
  }
  prefix = '';
  return prefix;
};
