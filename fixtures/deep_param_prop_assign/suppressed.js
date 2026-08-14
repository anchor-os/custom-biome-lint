export function sameLineForm(accum, id, priceBands) {
  accum.tours[id].priceBands = priceBands; // custom-biome-ignore-line deep-param-prop-assign
}

export function nextLineForm(accum, id, priceBands) {
  // custom-biome-ignore-next-line deep-param-prop-assign
  accum.tours[id].priceBands = priceBands;
}

export function withJustification(accum, id, priceBands) {
  // custom-biome-ignore-next-line deep-param-prop-assign -- accumulator is built locally in this reducer
  accum.tours[id].priceBands = priceBands;
}

// The overlap case, suppressed for both rules with one marker.
export const bothRules = item => {
  // custom-biome-ignore-next-line bare-arrow-param-prop-assign, deep-param-prop-assign
  item.a.b = 1;
};
