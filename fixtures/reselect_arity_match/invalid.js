import { createSelector } from 'reselect';

const selectUsers = state => state.users;
const selectFilter = state => state.filter;

// Two inputs, one parameter: the filter is silently dropped.
export const selectTooFewParams = createSelector(selectUsers, selectFilter, users => users);

// One input, two parameters: the second is always undefined.
export const selectTooManyParams = createSelector(selectUsers, (users, filter) => users);

// Same mismatch through a function expression and a namespaced callee.
export const selectViaFunction = createSelector(selectUsers, selectFilter, function (users) {
  return users;
});
