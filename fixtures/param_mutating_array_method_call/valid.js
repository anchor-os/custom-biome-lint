function transform(list) {
  return list.map(x => x * 2).filter(Boolean);
}

function getPusher(list) {
  return list.push;
}

function safeAdd(list, item) {
  const copy = [...list];
  copy.push(item);
  return copy;
}

function aliased(list, item) {
  const local = list;
  local.push(item);
}

function immutableSet(product) {
  return product.set('name', 'x');
}

function bracketForm(list, item) {
  list['push'](item);
}

function onLocal(arr) {
  const arr2 = [1, 2];
  arr2.push(3);
}
