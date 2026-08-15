export function inCallback(items) {
  items.forEach(item => {
    for (let i = 0; i < item.children.length; i++) {
      visit(item.children[i]);
    }
  });
}

export function inTry(list) {
  try {
    for (let i = 0; i < list.length; i++) {
      process(list[i]);
    }
  } catch (e) {
    handle(e);
  }
}

export function labeled(data) {
  outer: for (let i = 0; i < data.length; i++) {
    step(data[i]);
  }
}
