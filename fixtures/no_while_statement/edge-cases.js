export function nested(list) {
  while (list.length) {
    while (more()) {
      step();
    }
  }
}

export function inCallback(items) {
  items.forEach(item => {
    while (item.pending()) {
      item.advance();
    }
  });
}

export function labeled(data) {
  outer: while (data.length) {
    step(data);
  }
}
