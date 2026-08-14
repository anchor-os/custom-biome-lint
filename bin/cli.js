#!/usr/bin/env node
'use strict';

const path = require('path');
const { spawn } = require('child_process');
const {
  detectLibc,
  resolvePackageName,
  binaryName,
  unsupportedPlatformMessage,
  missingPackageMessage,
} = require('./platform');

const FORWARDED_SIGNALS = ['SIGINT', 'SIGTERM', 'SIGHUP'];

// `overridePath` is CUSTOM_BIOME_LINT_BIN — an explicit escape hatch for
// contributors running a locally built binary (via `npm run build:native`)
// through this launcher without a matching platform package installed.
// `libc` is 'glibc' | 'musl' on Linux and null elsewhere. It is a parameter
// with a default rather than probed inside the resolution logic so tests can
// resolve either flavor from any host; the default keeps every existing caller
// (and every non-Linux platform, where it is null) behaving as before.
function resolveBinaryPath(platform, arch, overridePath, libc = detectLibc(platform)) {
  if (overridePath) {
    return { binPath: overridePath };
  }

  const packageName = resolvePackageName(platform, arch, libc);
  if (!packageName) {
    return { error: unsupportedPlatformMessage(platform, arch, libc) };
  }

  let packageJsonPath;
  try {
    packageJsonPath = require.resolve(`${packageName}/package.json`);
  } catch {
    return { error: missingPackageMessage(platform, arch, packageName, libc) };
  }

  return {
    binPath: path.join(path.dirname(packageJsonPath), 'bin', binaryName(platform)),
  };
}

function run(platform, arch, args, { spawnFn = spawn, env = process.env } = {}) {
  // Detected here, from the same `env` the tests inject, so a
  // CUSTOM_BIOME_LINT_LIBC override honors the injected environment rather
  // than the real process environment.
  const { binPath, error } = resolveBinaryPath(
    platform,
    arch,
    env.CUSTOM_BIOME_LINT_BIN,
    detectLibc(platform, { env })
  );
  if (error) {
    console.error(error);
    process.exitCode = 1;
    return;
  }

  const child = spawnFn(binPath, args, { stdio: 'inherit' });

  const forward = (signal) => child.kill(signal);
  FORWARDED_SIGNALS.forEach((signal) => process.on(signal, forward));

  const stopForwarding = () => {
    FORWARDED_SIGNALS.forEach((signal) => process.removeListener(signal, forward));
  };

  child.on('error', (err) => {
    stopForwarding();
    console.error(`custom-biome-lint: failed to run binary at ${binPath}\n${err.message}`);
    process.exitCode = 1;
  });

  child.on('exit', (code, signal) => {
    stopForwarding();
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exitCode = code ?? 1;
  });
}

if (require.main === module) {
  run(process.platform, process.arch, process.argv.slice(2));
}

module.exports = { resolveBinaryPath, run };
