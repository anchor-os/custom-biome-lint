'use strict';

// Maps `${process.platform}-${process.arch}` to the optional platform
// package that carries the precompiled binary for that target.
const PLATFORM_PACKAGES = {
  'darwin-arm64': 'custom-biome-lint-darwin-arm64',
  'darwin-x64': 'custom-biome-lint-darwin-x64',
  'linux-arm64': 'custom-biome-lint-linux-arm64',
  'linux-x64': 'custom-biome-lint-linux-x64',
  'win32-arm64': 'custom-biome-lint-win32-arm64',
  'win32-x64': 'custom-biome-lint-win32-x64',
};

const SUPPORTED_PLATFORMS = [
  { platform: 'darwin', arch: 'arm64', label: 'macOS arm64' },
  { platform: 'darwin', arch: 'x64', label: 'macOS x64' },
  { platform: 'linux', arch: 'arm64', label: 'Linux arm64' },
  { platform: 'linux', arch: 'x64', label: 'Linux x64' },
  { platform: 'win32', arch: 'arm64', label: 'Windows arm64' },
  { platform: 'win32', arch: 'x64', label: 'Windows x64' },
];

function resolvePackageName(platform, arch) {
  return PLATFORM_PACKAGES[`${platform}-${arch}`] || null;
}

function binaryName(platform) {
  return platform === 'win32' ? 'custom-biome-lint.exe' : 'custom-biome-lint';
}

function unsupportedPlatformMessage(platform, arch) {
  return [
    'custom-biome-lint does not have a prebuilt binary for:',
    `  platform: ${platform}`,
    `  architecture: ${arch}`,
    '',
    'Supported platforms:',
    ...SUPPORTED_PLATFORMS.map((p) => `  ${p.label}`),
    '',
    'If you are developing custom-biome-lint from source, use the source-build',
    'workflow documented in docs/USE_AS_GIT_SUBMODULE.md (run `npm run build:native`).',
  ].join('\n');
}

function missingPackageMessage(platform, arch, packageName) {
  return [
    `custom-biome-lint could not find its prebuilt binary package "${packageName}"`,
    `for platform: ${platform}, architecture: ${arch}.`,
    '',
    'This usually means the optional dependency failed to install. Try:',
    '  npm install',
    '',
    'If you are developing custom-biome-lint from source, use the source-build',
    'workflow documented in docs/USE_AS_GIT_SUBMODULE.md (run `npm run build:native`).',
  ].join('\n');
}

module.exports = {
  PLATFORM_PACKAGES,
  SUPPORTED_PLATFORMS,
  resolvePackageName,
  binaryName,
  unsupportedPlatformMessage,
  missingPackageMessage,
};
