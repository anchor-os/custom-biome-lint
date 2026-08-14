export function sameLineForm({ b = '' }) {
  b = 'x'; // custom-biome-ignore-line destructure-default-param-assign
  return b;
}

export function nextLineForm({ b = '' }) {
  // custom-biome-ignore-next-line destructure-default-param-assign
  b = 'x';
  return b;
}

export function withJustification({ b = '' }) {
  // custom-biome-ignore-next-line destructure-default-param-assign -- normalising a legacy default
  b = 'x';
  return b;
}

export function bareSuppression({ b = '' }) {
  // custom-biome-ignore-next-line
  b = 'x';
  return b;
}
