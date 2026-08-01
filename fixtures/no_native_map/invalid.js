import { List } from 'immutable';

export const cache = new Map();

export function index(rows) {
  const lookup = new Map();
  rows.forEach(row => lookup.set(row.id, row));
  return lookup;
}

export const asList = List([1, 2]);
