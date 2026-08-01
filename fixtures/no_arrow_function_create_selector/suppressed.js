import { createSelector } from 'reselect';

const selectUsers = state => state.users;

// biome-ignore-next-line no-arrow-function-create-selector
export const selectFirstUser = () => createSelector(selectUsers, users => users.first());

export const selectLastUser = () => createSelector(selectUsers, users => users.last()); // biome-ignore-line no-arrow-function-create-selector
