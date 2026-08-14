// An arrow whose single parameter has no parentheses binds it directly under
// the arrow, with no JsParameters node — the shape Biome's noParameterAssign
// does not see as a parameter at all.

export const setX = item => {
  item.x = 1;
};

export const assignLane = booking => {
  booking.laneIndex = 0;
};

export const setMaxLanes = event => {
  event.maxLanes = 4;
};

export const bracketForm = row => {
  row['total'] = 0;
};
