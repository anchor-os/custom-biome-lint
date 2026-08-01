import { Map, List } from 'immutable';

export const emptyState = Map();

export const withDefaults = Map({ id: null, name: '' });

export const asList = List([Map(), Map()]);
