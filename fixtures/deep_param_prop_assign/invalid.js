// Plain parameters mutated two or more levels deep — the chains Biome's
// noParameterAssign stops tracking after the first hop.

export function stampDocument(acc, instance, documentName) {
  acc[instance.id][documentName] = Date.now();
}

export function collectPriceBands(accum, bookingTypeId, priceBands) {
  accum.tours[bookingTypeId].priceBands = priceBands;
}

export const total = (acc, item) => {
  acc[item.id].total += item.total;
};

export function threeDeep(state, id) {
  state.tours[id].priceBands = {};
}
