import { createSelector } from 'reselect';

const selectUsers = state => state.users;
const selectFilter = state => state.filter;
const selectPage = state => state.page;

export const selectVisibleUsers = createSelector(
  selectUsers,
  selectFilter,
  (users, filter) => users.filter(user => user.type === filter)
);

export const selectPagedUsers = createSelector(
  selectUsers,
  selectFilter,
  selectPage,
  function paged(users, filter, page) {
    return users.filter(user => user.type === filter).slice(page * 10);
  }
);

// A selector passed by reference has no visible arity, so it is not checked.
export const selectDelegated = createSelector(selectUsers, selectFilter, mergeUsers);

function mergeUsers(users, filter) {
  return { users, filter };
}
