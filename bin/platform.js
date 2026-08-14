'use strict';

// Maps a resolution key to the optional platform package that carries the
// precompiled binary for that target. The key is `${process.platform}-${process.arch}`
// everywhere except musl-based Linux, which gets a `-musl` suffix: glibc and
// musl need different binaries, but `process.platform` is `"linux"` for both,
// so the libc flavor has to be part of the key (see `detectLibc`).
const PLATFORM_PACKAGES = {
  'darwin-arm64': 'custom-biome-lint-darwin-arm64',
  'darwin-x64': 'custom-biome-lint-darwin-x64',
  'linux-arm64': 'custom-biome-lint-linux-arm64',
  'linux-arm64-musl': 'custom-biome-lint-linux-arm64-musl',
  'linux-x64': 'custom-biome-lint-linux-x64',
  'linux-x64-musl': 'custom-biome-lint-linux-x64-musl',
  'win32-arm64': 'custom-biome-lint-win32-arm64',
  'win32-x64': 'custom-biome-lint-win32-x64',
};

const SUPPORTED_PLATFORMS = [
  { platform: 'darwin', arch: 'arm64', label: 'macOS arm64' },
  { platform: 'darwin', arch: 'x64', label: 'macOS x64' },
  { platform: 'linux', arch: 'arm64', libc: 'glibc', label: 'Linux arm64 (glibc)' },
  { platform: 'linux', arch: 'arm64', libc: 'musl', label: 'Linux arm64 (musl, e.g. Alpine)' },
  { platform: 'linux', arch: 'x64', libc: 'glibc', label: 'Linux x64 (glibc)' },
  { platform: 'linux', arch: 'x64', libc: 'musl', label: 'Linux x64 (musl, e.g. Alpine)' },
  { platform: 'win32', arch: 'arm64', label: 'Windows arm64' },
  { platform: 'win32', arch: 'x64', label: 'Windows x64' },
];

// Escape hatch for a machine where libc detection gets it wrong (a patched or
// stripped `process.report`, an unusual distro). Same spirit as
// CUSTOM_BIOME_LINT_BIN: an explicit override always beats detection.
const LIBC_ENV_VAR = 'CUSTOM_BIOME_LINT_LIBC';

// Reads Node's own account of the libc it is linked against. Returns the
// runtime glibc version string, `''` when the report exists but reports no
// glibc (that is what a musl build looks like), or `null` when the report
// could not be read at all — `process.report` is optional API surface and can
// be missing, patched away by an embedder, or throw. `null` deliberately does
// not mean musl: see `detectLibc`.
function readGlibcVersionReport() {
  try {
    const header = process.report?.getReport()?.header;
    if (!header) {
      return null;
    }
    return header.glibcVersionRuntime ?? '';
  } catch {
    return null;
  }
}

// Returns 'glibc' | 'musl' on Linux, and `null` on every other platform —
// nothing else in this repo ships more than one libc flavor per platform, and
// on macOS `glibcVersionRuntime` is absent for the ordinary reason that macOS
// has no glibc, which must not be read as "musl".
//
// The report reader and env are parameters (not read from globals deep inside)
// so tests can drive every branch without mutating process state.
//
// An unreadable report falls back to 'glibc', never 'musl': glibc Linux is the
// flavor that already worked before musl packages existed, so an inconclusive
// probe must resolve exactly the way it resolved before this function existed.
// Guessing 'musl' would turn a working glibc install into a
// missing-package failure.
function detectLibc(
  platform,
  { env = process.env, readGlibcVersion = readGlibcVersionReport } = {}
) {
  if (platform !== 'linux') {
    return null;
  }

  const override = env[LIBC_ENV_VAR];
  if (override === 'glibc' || override === 'musl') {
    return override;
  }

  const glibcVersion = readGlibcVersion();
  if (glibcVersion === null) {
    return 'glibc';
  }
  return glibcVersion === '' ? 'musl' : 'glibc';
}

function resolvePackageName(platform, arch, libc) {
  const key =
    platform === 'linux' && libc === 'musl'
      ? `${platform}-${arch}-musl`
      : `${platform}-${arch}`;
  return PLATFORM_PACKAGES[key] || null;
}

function binaryName(platform) {
  return platform === 'win32' ? 'custom-biome-lint.exe' : 'custom-biome-lint';
}

function unsupportedPlatformMessage(platform, arch, libc) {
  return [
    'custom-biome-lint does not have a prebuilt binary for:',
    `  platform: ${platform}`,
    `  architecture: ${arch}`,
    ...(libc ? [`  libc: ${libc}`] : []),
    '',
    'Supported platforms:',
    ...SUPPORTED_PLATFORMS.map((p) => `  ${p.label}`),
    '',
    'If you are developing custom-biome-lint from source, use the source-build',
    'workflow documented in docs/USE_AS_GIT_SUBMODULE.md (run `npm run build:native`).',
  ].join('\n');
}

function missingPackageMessage(platform, arch, packageName, libc) {
  return [
    `custom-biome-lint could not find its prebuilt binary package "${packageName}"`,
    `for platform: ${platform}, architecture: ${arch}${libc ? `, libc: ${libc}` : ''}.`,
    '',
    'This usually means the optional dependency failed to install. Try:',
    '  npm install',
    '',
    // Only reachable on Linux, where the libc flavor was detected rather than
    // read off process.platform — so it is the one part of the resolution key
    // that can be wrong on an otherwise healthy install.
    ...(libc
      ? [
          `Detected libc: ${libc}. If that is wrong for this machine, set`,
          `${LIBC_ENV_VAR}=glibc (or musl) to override the detection.`,
          '',
        ]
      : []),
    'If you are developing custom-biome-lint from source, use the source-build',
    'workflow documented in docs/USE_AS_GIT_SUBMODULE.md (run `npm run build:native`).',
  ].join('\n');
}

module.exports = {
  PLATFORM_PACKAGES,
  SUPPORTED_PLATFORMS,
  LIBC_ENV_VAR,
  detectLibc,
  resolvePackageName,
  binaryName,
  unsupportedPlatformMessage,
  missingPackageMessage,
};
