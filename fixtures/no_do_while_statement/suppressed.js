export function attempt() {
  do { // custom-biome-ignore-line no-do-while-statement
    step();
  } while (!success());
}

export function retry(action, attempts) {
  let remaining = attempts;
  // custom-biome-ignore-next-line no-do-while-statement
  do {
    action();
    remaining -= 1;
  } while (remaining > 0);
}
