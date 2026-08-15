export function attempt() {
  do {
    step();
  } while (!success());
}

export function read() {
  do {
    next();
  } while (hasMore());
}

export function retry(action, attempts) {
  let remaining = attempts;
  do {
    action();
    remaining -= 1;
  } while (remaining > 0);
}
