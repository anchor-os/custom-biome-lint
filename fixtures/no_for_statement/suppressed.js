export function iterate(items) {
  for (let i = 0; i < items.length; i++) { // custom-biome-ignore-line no-for-statement
    process(items[i]);
  }
}

export function reverse(items) {
  // custom-biome-ignore-next-line no-for-statement
  for (let i = items.length - 1; i >= 0; i--) {
    process(items[i]);
  }
}
