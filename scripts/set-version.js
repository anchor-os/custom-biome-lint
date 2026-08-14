#!/usr/bin/env node
'use strict';

// Keeps Cargo.toml, package.json, package-lock.json, and every
// npm/<platform>/package.json at the same version for a release. Run before
// `cargo build --release` and
// before packaging the platform npm packages, so every published artifact
// (main package, optionalDependencies, platform packages, `--version`
// output) agrees.
//
// Usage: node scripts/set-version.js <version>
//        node scripts/set-version.js v0.1.2   (leading "v" is stripped)

const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..');
// Every platform package this repo publishes. A platform missing from this
// list is silently left at whatever version it was last written with — which
// then ships as a platform package whose version no longer matches the main
// package's optionalDependencies pin, so its install can never resolve. Add
// new platforms here in the same commit that adds their npm/<platform>/
// directory.
const PLATFORMS = [
  'darwin-arm64',
  'darwin-x64',
  'linux-arm64',
  'linux-arm64-musl',
  'linux-x64',
  'linux-x64-musl',
  'win32-arm64',
  'win32-x64',
];

function fail(message) {
  console.error(`set-version: ${message}`);
  process.exit(1);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

// Reads and validates every file we're about to touch, and returns a list of
// { filePath, contents } write jobs — but performs no writes itself. Keeping
// every read/validate step ahead of every write means a missing, malformed,
// or unwritable manifest fails before anything is changed, instead of
// leaving the repo with (say) Cargo.toml bumped but package.json untouched.
function preflight(version) {
  const writes = [];

  const cargoPath = path.join(ROOT, 'Cargo.toml');
  const cargoContents = fs.readFileSync(cargoPath, 'utf8');
  const versionPattern = /^version = "[^"]*"/m;
  if (!versionPattern.test(cargoContents)) {
    fail(`could not find a "version = ..." line in ${cargoPath}`);
  }
  writes.push({
    filePath: cargoPath,
    contents: cargoContents.replace(versionPattern, `version = "${version}"`),
  });

  const rootPackagePath = path.join(ROOT, 'package.json');
  const rootPkg = readJson(rootPackagePath);
  rootPkg.version = version;
  for (const platform of PLATFORMS) {
    const depName = `custom-biome-lint-${platform}`;
    if (!rootPkg.optionalDependencies || !(depName in rootPkg.optionalDependencies)) {
      fail(`root package.json is missing optionalDependencies["${depName}"]`);
    }
    rootPkg.optionalDependencies[depName] = version;
  }
  writes.push({ filePath: rootPackagePath, contents: `${JSON.stringify(rootPkg, null, 2)}\n` });

  for (const platform of PLATFORMS) {
    const packagePath = path.join(ROOT, 'npm', platform, 'package.json');
    const pkg = readJson(packagePath);
    pkg.version = version;
    writes.push({ filePath: packagePath, contents: `${JSON.stringify(pkg, null, 2)}\n` });
  }

  // package-lock.json records the root version in two places plus its own copy
  // of optionalDependencies, and `npm ci` fails when those disagree with
  // package.json. Edited directly rather than regenerated with `npm install
  // --package-lock-only`, because that resolves every optionalDependency
  // against the registry and the platform packages for a version being
  // released do not exist there yet — the release publishes them.
  const lockPath = path.join(ROOT, 'package-lock.json');
  if (fs.existsSync(lockPath)) {
    const lock = readJson(lockPath);
    const rootEntry = lock.packages && lock.packages[''];
    if (!rootEntry) {
      fail('package-lock.json has no packages[""] entry (expected lockfileVersion 2 or 3)');
    }
    lock.version = version;
    rootEntry.version = version;
    if (rootEntry.optionalDependencies) {
      // Assigned unconditionally, not only for keys already present: a newly
      // added platform must appear in the lockfile too, or `npm ci` rejects
      // the tree as out of sync with package.json.
      for (const platform of PLATFORMS) {
        rootEntry.optionalDependencies[`custom-biome-lint-${platform}`] = version;
      }
    }
    writes.push({ filePath: lockPath, contents: `${JSON.stringify(lock, null, 2)}\n` });
  }

  // fs.accessSync throws if the path is missing or not writable, catching an
  // unwritable manifest (e.g. read-only file) before any write happens.
  for (const { filePath } of writes) {
    fs.accessSync(filePath, fs.constants.W_OK);
  }

  return writes;
}

function main() {
  const rawVersion = process.argv[2];
  if (!rawVersion) {
    fail('usage: node scripts/set-version.js <version>');
  }
  const version = rawVersion.replace(/^v/, '');
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    fail(`"${version}" does not look like a semver version (expected x.y.z)`);
  }

  let writes;
  try {
    writes = preflight(version);
  } catch (err) {
    fail(`preflight failed, nothing was written: ${err.message}`);
    return;
  }

  for (const { filePath, contents } of writes) {
    fs.writeFileSync(filePath, contents);
  }

  console.log(
    `set-version: updated Cargo.toml, package.json, package-lock.json, and npm/*/package.json to ${version}`
  );
}

main();
