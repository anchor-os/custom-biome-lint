#!/usr/bin/env node
'use strict';

// Stand-in for the precompiled Rust binary, used by bin/cli.test.js to
// verify the launcher forwards args/stdio/exit codes without needing a
// real platform package installed.
const args = process.argv.slice(2);

if (args[0] === '--help') {
  process.stdout.write('usage: custom-biome-lint [options] <path>\n');
  process.exit(0);
}

if (args[0] === '--version') {
  process.stdout.write('0.2.0\n');
  process.exit(0);
}

if (args.includes('--fail')) {
  process.stderr.write('stderr-marker\n');
  process.exit(7);
}

process.stdout.write(`ARGS:${JSON.stringify(args)}\n`);
process.stdout.write('stdout-marker\n');
process.stderr.write('stderr-marker\n');
process.exit(0);
