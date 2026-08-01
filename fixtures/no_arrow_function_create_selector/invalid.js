import { createSelector } from 'reselect';

const selectUsers = state => state.users;
const selectFilter = state => state.filter;

// Rebuilt on every call, so the memoized cache is always empty.
export const selectVisibleUsers = () =>
  createSelector(selectUsers, selectFilter, (users, filter) =>
    users.filter(user => user.type === filter)
  );

export const selectFirstUser = () => createSelector(selectUsers, users => users.first());
