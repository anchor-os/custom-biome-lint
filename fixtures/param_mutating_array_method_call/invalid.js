export const buildProductsData = ({ productsData }, product) => {
  productsData.push(product);
};

function collect(accum, item) {
  accum.push(item);
  return accum;
}

export const addFavorite = ({ favorites }, item) => {
  favorites.push(item);
};

function addToGroup(accum, key, item) {
  accum[key].items.push(item);
}

function sortInPlace(list) {
  list.sort();
}

function removeFirst(list) {
  list.shift();
}

function optionalChaining(list, item) {
  list?.push(item);
}
