'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const {
  detectLibc,
  resolvePackageName,
  binaryName,
  unsupportedPlatformMessage,
  missingPackageMessage,
  SUPPORTED_PLATFORMS,
} = require('./platform');

// The libc suffix is part of the package name only for musl Linux; every other
// entry keeps the plain `<platform>-<arch>` shape.
function expectedPackageName({ platform, arch, libc }) {
  const suffix = libc === 'musl' ? '-musl' : '';
  return `custom-biome-lint-${platform}-${arch}${suffix}`;
}

test('maps every supported platform/arch/libc triple to its platform package', () => {
  for (const entry of SUPPORTED_PLATFORMS) {
    assert.equal(
      resolvePackageName(entry.platform, entry.arch, entry.libc),
      expectedPackageName(entry)
    );
  }
});

test('returns null for an unsupported platform/arch pair', () => {
  assert.equal(resolvePackageName('freebsd', 'x64'), null);
  assert.equal(resolvePackageName('darwin', 'ia32'), null);
});

test('musl resolves to the -musl package on both Linux architectures', () => {
  assert.equal(resolvePackageName('linux', 'x64', 'musl'), 'custom-biome-lint-linux-x64-musl');
  assert.equal(resolvePackageName('linux', 'arm64', 'musl'), 'custom-biome-lint-linux-arm64-musl');
});

test('glibc (and an absent libc) still resolves to the original Linux packages', () => {
  assert.equal(resolvePackageName('linux', 'x64', 'glibc'), 'custom-biome-lint-linux-x64');
  assert.equal(resolvePackageName('linux', 'arm64', 'glibc'), 'custom-biome-lint-linux-arm64');
  // No libc argument at all: the pre-musl call shape must keep resolving the
  // glibc package, so an older caller cannot be silently redirected.
  assert.equal(resolvePackageName('linux', 'x64'), 'custom-biome-lint-linux-x64');
  assert.equal(resolvePackageName('linux', 'arm64'), 'custom-biome-lint-linux-arm64');
});

test('a musl libc never affects non-Linux resolution', () => {
  // process.report reports no glibc on macOS and Windows for the ordinary
  // reason that neither has glibc; a stray 'musl' must not invent a package
  // name like custom-biome-lint-darwin-arm64-musl.
  assert.equal(resolvePackageName('darwin', 'arm64', 'musl'), 'custom-biome-lint-darwin-arm64');
  assert.equal(resolvePackageName('darwin', 'x64', 'musl'), 'custom-biome-lint-darwin-x64');
  assert.equal(resolvePackageName('win32', 'arm64', 'musl'), 'custom-biome-lint-win32-arm64');
  assert.equal(resolvePackageName('win32', 'x64', 'musl'), 'custom-biome-lint-win32-x64');
});

test('detectLibc returns null off Linux, whatever the report says', () => {
  const readGlibcVersion = () => '';
  const env = {};
  assert.equal(detectLibc('darwin', { env, readGlibcVersion }), null);
  assert.equal(detectLibc('win32', { env, readGlibcVersion }), null);
  assert.equal(detectLibc('freebsd', { env, readGlibcVersion }), null);
});

test('detectLibc reads musl from a report with no runtime glibc version', () => {
  assert.equal(detectLibc('linux', { env: {}, readGlibcVersion: () => '' }), 'musl');
});

test('detectLibc reads glibc from a report carrying a runtime glibc version', () => {
  assert.equal(detectLibc('linux', { env: {}, readGlibcVersion: () => '2.36' }), 'glibc');
});

test('detectLibc falls back to glibc when the report cannot be read', () => {
  // null means "process.report was missing, patched, or threw". Guessing musl
  // here would break glibc hosts that resolve correctly today, so the
  // inconclusive case must keep the pre-musl behavior.
  assert.equal(detectLibc('linux', { env: {}, readGlibcVersion: () => null }), 'glibc');
});

test('CUSTOM_BIOME_LINT_LIBC overrides detection in both directions', () => {
  assert.equal(
    detectLibc('linux', { env: { CUSTOM_BIOME_LINT_LIBC: 'musl' }, readGlibcVersion: () => '2.36' }),
    'musl'
  );
  assert.equal(
    detectLibc('linux', { env: { CUSTOM_BIOME_LINT_LIBC: 'glibc' }, readGlibcVersion: () => '' }),
    'glibc'
  );
});

test('an unrecognized CUSTOM_BIOME_LINT_LIBC value is ignored, not fatal', () => {
  // A typo in the escape hatch must fall through to detection rather than
  // resolving a package name that does not exist.
  assert.equal(
    detectLibc('linux', { env: { CUSTOM_BIOME_LINT_LIBC: 'MUSL' }, readGlibcVersion: () => '' }),
    'musl'
  );
  assert.equal(
    detectLibc('linux', { env: { CUSTOM_BIOME_LINT_LIBC: '' }, readGlibcVersion: () => '2.36' }),
    'glibc'
  );
});

test('uses the .exe suffix only on win32', () => {
  assert.equal(binaryName('win32'), 'custom-biome-lint.exe');
  assert.equal(binaryName('darwin'), 'custom-biome-lint');
  assert.equal(binaryName('linux'), 'custom-biome-lint');
});

test('unsupported platform message names the platform, arch, and supported list', () => {
  const message = unsupportedPlatformMessage('freebsd', 'x64');
  assert.match(message, /platform: freebsd/);
  assert.match(message, /architecture: x64/);
  assert.match(message, /macOS arm64/);
  assert.match(message, /Windows x64/);
});

test('unsupported platform message lists both Linux libc flavors', () => {
  const message = unsupportedPlatformMessage('linux', 'ia32', 'musl');
  assert.match(message, /libc: musl/);
  assert.match(message, /Linux x64 \(glibc\)/);
  assert.match(message, /Linux x64 \(musl, e\.g\. Alpine\)/);
  assert.match(message, /Linux arm64 \(glibc\)/);
  assert.match(message, /Linux arm64 \(musl, e\.g\. Alpine\)/);
});

test('unsupported platform message omits the libc line off Linux', () => {
  assert.doesNotMatch(unsupportedPlatformMessage('freebsd', 'x64', null), /^ {2}libc:/m);
});

test('missing package message names the detected libc and the override', () => {
  const message = missingPackageMessage(
    'linux',
    'x64',
    'custom-biome-lint-linux-x64-musl',
    'musl'
  );
  assert.match(message, /custom-biome-lint-linux-x64-musl/);
  assert.match(message, /libc: musl/);
  assert.match(message, /CUSTOM_BIOME_LINT_LIBC=glibc \(or musl\)/);
});

test('missing package message says nothing about libc off Linux', () => {
  const message = missingPackageMessage('darwin', 'arm64', 'custom-biome-lint-darwin-arm64', null);
  assert.doesNotMatch(message, /libc/);
  assert.doesNotMatch(message, /CUSTOM_BIOME_LINT_LIBC/);
});
