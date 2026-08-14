export const sameLineForm = item => {
  item.x = 1; // custom-biome-ignore-line bare-arrow-param-prop-assign
};

export const nextLineForm = item => {
  // custom-biome-ignore-next-line bare-arrow-param-prop-assign
  item.x = 1;
};

export const withJustification = item => {
  // custom-biome-ignore-next-line bare-arrow-param-prop-assign -- hot path, copy measured slower
  item.x = 1;
};

// One marker can carry both rule names, which is how the deliberate overlap
// with deep-param-prop-assign is meant to be suppressed.
export const bothRules = item => {
  // custom-biome-ignore-next-line bare-arrow-param-prop-assign, deep-param-prop-assign
  item.a.b = 1;
};
