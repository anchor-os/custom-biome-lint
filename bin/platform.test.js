'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const {
  resolvePackageName,
  binaryName,
  unsupportedPlatformMessage,
  SUPPORTED_PLATFORMS,
} = require('./platform');

test('maps every supported platform/arch pair to its platform package', () => {
  for (const { platform, arch } of SUPPORTED_PLATFORMS) {
    assert.equal(resolvePackageName(platform, arch), `custom-biome-lint-${platform}-${arch}`);
  }
});

test('returns null for an unsupported platform/arch pair', () => {
  assert.equal(resolvePackageName('freebsd', 'x64'), null);
  assert.equal(resolvePackageName('darwin', 'ia32'), null);
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
