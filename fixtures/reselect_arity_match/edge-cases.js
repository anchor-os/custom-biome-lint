import { createSelector } from 'reselect';

const selectUsers = state => state.users;
const selectFilter = state => state.filter;

// A selector passed by reference has no visible parameter list at the call
// site, so it is not checked -- outside what this rule can see, not a
// false negative.
export const selectDelegated = createSelector(selectUsers, selectFilter, mergeUsers);

function mergeUsers(users, filter) {
  return { users, filter };
}

// Fewer than 2 arguments: nothing to check, never flagged.
export const selectSingleInput = createSelector(selectUsers);

// Namespaced callee with a real mismatch IS still checked: both Identifier
// and MemberExpression callees are matched.
export const selectViaNamespace = Reselect.createSelector(selectUsers, selectFilter, users => users);

// Concise single-param arrow counts as exactly 1 parameter.
export const selectConciseSingleParam = createSelector(selectUsers, users => users);
