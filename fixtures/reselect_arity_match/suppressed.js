import { createSelector } from 'reselect';

const selectUsers = state => state.users;
const selectFilter = state => state.filter;

// The extra input is consumed for cache invalidation only, never read.
export const selectUsersOnly = createSelector(selectUsers, selectFilter, users => users); // custom-biome-ignore-line reselect-arity-match

// custom-biome-ignore-next-line reselect-arity-match
export const selectAgain = createSelector(selectUsers, selectFilter, users => users);
