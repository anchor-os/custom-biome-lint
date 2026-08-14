'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { resolveBinaryPath, run } = require('./cli');

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

test('CUSTOM_BIOME_LINT_BIN wins over libc detection', () => {
  // A contributor pointing the launcher at a locally built binary must not be
  // told their machine's libc flavor has no platform package.
  const { error, binPath } = resolveBinaryPath('linux', 'x64', '/some/local/binary', 'musl');
  assert.equal(error, undefined);
  assert.equal(binPath, '/some/local/binary');
});

test('resolveBinaryPath looks for the -musl package when libc is musl', () => {
  const { error, binPath } = resolveBinaryPath('linux', 'arm64', undefined, 'musl');
  assert.equal(binPath, undefined);
  assert.match(error, /"custom-biome-lint-linux-arm64-musl"/);
});

// Drives `run`'s own resolution rather than resolveBinaryPath's, because the
// libc flavor is detected inside `run` from the injected env — the wiring that
// would silently break if `run` ever read process.env directly instead.
// Neither platform package is installed in this repo, so both cases land on the
// missing-package error, which is what names the package that was looked for.
function resolutionErrorFrom(platform, arch, env) {
  const originalConsoleError = console.error;
  const originalExitCode = process.exitCode;
  const lines = [];
  console.error = (message) => lines.push(String(message));
  try {
    run(platform, arch, [], {
      spawnFn: () => assert.fail('resolution was expected to fail before spawning'),
      env,
    });
  } finally {
    console.error = originalConsoleError;
    // `run` sets process.exitCode = 1 on a resolution failure; leaving it set
    // would fail the whole test file even with every assertion passing.
    process.exitCode = originalExitCode;
  }
  return lines.join('\n');
}

// npm's cpu gating means a platform package for an arch other than the host's
// can never be installed here, which keeps the tests below on the deterministic
// "missing package" path no matter which machine runs them.
const NON_HOST_ARCH = process.arch === 'x64' ? 'arm64' : 'x64';

test('run resolves the musl package when CUSTOM_BIOME_LINT_LIBC forces musl', () => {
  const error = resolutionErrorFrom('linux', NON_HOST_ARCH, { CUSTOM_BIOME_LINT_LIBC: 'musl' });
  assert.match(error, new RegExp(`"custom-biome-lint-linux-${NON_HOST_ARCH}-musl"`));
});

test('run resolves the glibc package when CUSTOM_BIOME_LINT_LIBC forces glibc', () => {
  const error = resolutionErrorFrom('linux', NON_HOST_ARCH, { CUSTOM_BIOME_LINT_LIBC: 'glibc' });
  assert.match(error, new RegExp(`"custom-biome-lint-linux-${NON_HOST_ARCH}"`));
  assert.doesNotMatch(error, /-musl/);
});

test('run never appends a libc suffix on a non-Linux platform', () => {
  const error = resolutionErrorFrom('darwin', NON_HOST_ARCH, { CUSTOM_BIOME_LINT_LIBC: 'musl' });
  assert.match(error, new RegExp(`"custom-biome-lint-darwin-${NON_HOST_ARCH}"`));
  assert.doesNotMatch(error, /-musl/);
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
