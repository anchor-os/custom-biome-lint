export function processAll(items) {
  return items.map(process);
}

export function drain(queue) {
  while (queue.length > 0) {
    process(queue.shift());
  }
}

export function retry(action, attempts) {
  let remaining = attempts;
  while (remaining > 0) {
    action();
    remaining -= 1;
  }
}
