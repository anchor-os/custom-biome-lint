#!/usr/bin/env node
'use strict';

const path = require('path');
const { spawn } = require('child_process');
const {
  resolvePackageName,
  binaryName,
  unsupportedPlatformMessage,
  missingPackageMessage,
} = require('./platform');

const FORWARDED_SIGNALS = ['SIGINT', 'SIGTERM', 'SIGHUP'];

// `overridePath` is CUSTOM_BIOME_LINT_BIN — an explicit escape hatch for
// contributors running a locally built binary (via `npm run build:native`)
// through this launcher without a matching platform package installed.
function resolveBinaryPath(platform, arch, overridePath) {
  if (overridePath) {
    return { binPath: overridePath };
  }

  const packageName = resolvePackageName(platform, arch);
  if (!packageName) {
    return { error: unsupportedPlatformMessage(platform, arch) };
  }

  let packageJsonPath;
  try {
    packageJsonPath = require.resolve(`${packageName}/package.json`);
  } catch {
    return { error: missingPackageMessage(platform, arch, packageName) };
  }

  return {
    binPath: path.join(path.dirname(packageJsonPath), 'bin', binaryName(platform)),
  };
}

function run(platform, arch, args, { spawnFn = spawn, env = process.env } = {}) {
  const { binPath, error } = resolveBinaryPath(platform, arch, env.CUSTOM_BIOME_LINT_BIN);
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
