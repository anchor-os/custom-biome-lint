export const sameLineForm = ({ payload }, val) => {
  payload.token = val; // custom-biome-ignore-line destructure-param-prop-assign
};

export const nextLineForm = ({ payload }, val) => {
  // custom-biome-ignore-next-line destructure-param-prop-assign
  payload.token = val;
};

export const withJustification = ({ payload }, val) => {
  // custom-biome-ignore-next-line destructure-param-prop-assign -- legacy saga, copy lands in the next PR
  payload.token = val;
};

export function deepChainSuppressed({ state }, id) {
  // custom-biome-ignore-next-line destructure-param-prop-assign
  state.tours[id].priceBands = {};
}
