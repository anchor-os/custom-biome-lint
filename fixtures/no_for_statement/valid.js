export function processAll(items) {
  return items.map(process);
}

export function logEach(list) {
  for (const item of list) {
    process(item);
  }
}

export function logKeys(obj) {
  for (const key in obj) {
    process(key);
  }
}
