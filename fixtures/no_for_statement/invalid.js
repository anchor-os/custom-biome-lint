export function iterate(items) {
  for (let i = 0; i < items.length; i++) {
    process(items[i]);
  }
}

export function stride(limits) {
  for (let i = 0; i < limits.length; i += 2) {
    use(limits[i]);
  }
}

export function reverse(items) {
  for (let i = items.length - 1; i >= 0; i--) {
    process(items[i]);
  }
}
