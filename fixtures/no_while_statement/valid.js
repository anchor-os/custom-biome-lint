export function processAll(items) {
  return items.map(process);
}

export function sum(list) {
  return list.reduce((acc, x) => acc + x, 0);
}

export async function collect(urls) {
  return Promise.all(urls.map(fetch));
}
