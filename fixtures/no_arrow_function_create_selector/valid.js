import { createSelector } from 'reselect';

const selectUsers = state => state.users;
const selectFilter = state => state.filter;

// Created once, so memoization holds across calls.
export const selectVisibleUsers = createSelector(
  selectUsers,
  selectFilter,
  (users, filter) => users.filter(user => user.type === filter)
);

// A deliberate factory is allowed: the make* prefix says a new selector per call
// is intentional (one instance per component).
export const makeSelectUserById = () =>
  createSelector(selectUsers, users => users.get(0));
