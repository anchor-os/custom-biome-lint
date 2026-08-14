// Real shapes from the saga/reducer code this rule was written for: an action
// or state object destructured in the parameter list, then mutated in place
// where the caller still holds the same reference.

export function* getSMSSessionsOfCustomersSaga({ payload }) {
  payload.token = yield call(getIdToken);
  yield put(payload);
}

export const renameObjKey = ({ obj, oldName, newName }) => {
  obj[newName] = obj[oldName];
  delete obj[oldName];
  return obj;
};

export function bracketForm({ acc }, key, value) {
  acc[key] = value;
}

export function chained({ state }, id) {
  state.tours[id].priceBands = {};
}

export const arrowForm = ({ payload }, val) => {
  payload.token = val;
};
