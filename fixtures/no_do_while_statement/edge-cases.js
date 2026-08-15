export function nested(list) {
  do {
    do {
      step();
    } while (inner(list));
  } while (outer(list));
}

export function inCallback(items) {
  items.forEach(item => {
    do {
      item.advance();
    } while (item.pending());
  });
}

export function labeled(data) {
  outer: do {
    step(data);
  } while (data.length);
}
