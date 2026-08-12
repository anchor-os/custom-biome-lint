'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { resolveBinaryPath } = require('./cli');

const CLI_PATH = path.join(__dirname, 'cli.js');
const FIXTURE_BIN = path.join(__dirname, '__fixtures__', 'fake-binary.js');

function runCli(args, envOverrides = {}) {
  return spawnSync(process.execPath, [CLI_PATH, ...args], {
    encoding: 'utf8',
    env: { ...process.env, CUSTOM_BIOME_LINT_BIN: FIXTURE_BIN, ...envOverrides },
  });
}

test('resolveBinaryPath errors clearly for an unsupported platform', () => {
  const { error, binPath } = resolveBinaryPath('freebsd', 'x64');
  assert.equal(binPath, undefined);
  assert.match(error, /does not have a prebuilt binary/);
  assert.match(error, /platform: freebsd/);
});

test('resolveBinaryPath errors clearly when the platform package is not installed', () => {
  // Pick a supported pair that can never match the host actually running this
  // test — real optionalDependencies mean the host's own platform package
  // (e.g. custom-biome-lint-linux-x64 on a Linux x64 machine) could really be
  // installed, which would make a hardcoded pair resolve successfully instead
  // of hitting "missing package".
  const [platform, arch] =
    process.platform === 'linux' && process.arch === 'x64'
      ? ['darwin', 'x64']
      : ['linux', 'x64'];
  const { error, binPath } = resolveBinaryPath(platform, arch);
  assert.equal(binPath, undefined);
  assert.match(error, /could not find its prebuilt binary package/);
});

test('resolveBinaryPath honors the CUSTOM_BIOME_LINT_BIN override', () => {
  const { error, binPath } = resolveBinaryPath('linux', 'x64', '/some/local/binary');
  assert.equal(error, undefined);
  assert.equal(binPath, '/some/local/binary');
});

test('forwards CLI arguments unchanged to the underlying binary', () => {
  const result = runCli(['src', '--format', 'json']);
  assert.equal(result.status, 0);
  assert.match(result.stdout, /ARGS:\["src","--format","json"\]/);
});

test('forwards the child exit code', () => {
  const ok = runCli([]);
  assert.equal(ok.status, 0);

  const failing = runCli(['--fail']);
  assert.equal(failing.status, 7);
});

test('forwards stdout', () => {
  const result = runCli([]);
  assert.match(result.stdout, /stdout-marker/);
});

test('forwards stderr', () => {
  const result = runCli([]);
  assert.match(result.stderr, /stderr-marker/);
});

test('forwards --help to the underlying binary', () => {
  const result = runCli(['--help']);
  assert.equal(result.status, 0);
  assert.match(result.stdout, /usage: custom-biome-lint/);
});

test('forwards --version to the underlying binary', () => {
  const result = runCli(['--version']);
  assert.equal(result.status, 0);
  assert.match(result.stdout, /0\.2\.0/);
});
