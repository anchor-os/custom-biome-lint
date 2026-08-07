import { createSelector } from 'reselect';

const selectUsers = state => state.users;

// Block body, not a concise expression body: not flagged. The rule's
// direct-parent check only recognizes a concise arrow.
const selectViaBlockBody = () => {
  return createSelector(selectUsers, users => users);
};

// The arrow is a call argument, not a declarator initializer: not flagged.
useMemo(() => createSelector(selectUsers, users => users), []);

// Callee is a member expression, not a bare identifier: not flagged.
const selectViaNamespace = () => reselect.createSelector(selectUsers, users => users);

// "make" not followed by an uppercase letter does not count as a factory
// name (the check is /^make[A-Z]/), so this one IS flagged despite starting
// with "make".
export const makeup = () => createSelector(selectUsers, users => users);
