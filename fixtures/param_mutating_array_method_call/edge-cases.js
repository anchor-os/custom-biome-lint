import { Map } from 'immutable';

export function immutablePush(pricelists, pricelistId) {
  return pricelists.push(Map({ pricelistId }));
}

function reduxFormFields(fields) {
  fields.push('');
}

function nestedShadow(accum) {
  function inner(accum) {
    accum.push(1);
  }
  inner(accum);
}
