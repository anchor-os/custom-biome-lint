export const buildProductsData = ({ productsData }, product) => {
  productsData.push(product); // custom-biome-ignore-line param-mutating-array-method-call
};

export const addFavorite = ({ favorites }, item) => {
  // custom-biome-ignore-next-line param-mutating-array-method-call
  favorites.push(item);
};
