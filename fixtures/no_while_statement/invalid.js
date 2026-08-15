export function drain(queue) {
  while (queue.length > 0) {
    process(queue.shift());
  }
}

export function countdown(n) {
  while (n > 0) {
    n -= 1;
  }
}

export function spin(condition) {
  while (condition()) {
    tick();
  }
}
