import { List } from 'immutable';

// A native Map is required here because the keys are DOM nodes, which
// Immutable.js cannot hash structurally.
export const nodeCache = new Map(); // custom-biome-ignore-line no-native-map

// custom-biome-ignore-next-line no-native-map
export const weakIndex = new Map();

export const asList = List([1, 2]);
