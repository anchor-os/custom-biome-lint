export function drain(queue) {
  while (queue.length > 0) { // custom-biome-ignore-line no-while-statement
    process(queue.shift());
  }
}

export function countdown(n) {
  // custom-biome-ignore-next-line no-while-statement
  while (n > 0) {
    n -= 1;
  }
}
