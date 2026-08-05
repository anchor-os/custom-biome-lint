#!/usr/bin/env node

const { spawnSync } = require('child_process');
const path = require('path');

const binName = process.platform === 'win32' ? 'custom-biome-lint.exe' : 'custom-biome-lint';
const binPath = path.join(__dirname, '..', 'target', 'release', binName);

const result = spawnSync(binPath, process.argv.slice(2), { stdio: 'inherit' });

if (result.error) {
  console.error(
    `custom-biome-lint: failed to run compiled binary at ${binPath}\n` +
      'Was the postinstall build step skipped? Try: cargo build --release'
  );
  process.exit(1);
}

process.exit(result.status ?? 1);
