import assert from 'node:assert/strict';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const usage = 'Usage: node scripts/verify-builtin-bundle.mjs --root <path> --source-release <tag>';

function parseOptions(args) {
  if (args.length !== 4) throw new Error(usage);
  const options = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const option = args[index];
    const value = args[index + 1];
    if (!['--root', '--source-release'].includes(option) || !value || options.has(option)) {
      throw new Error(usage);
    }
    options.set(option, value);
  }
  return {
    root: resolve(options.get('--root')),
    sourceRelease: options.get('--source-release').startsWith('v')
      ? options.get('--source-release')
      : `v${options.get('--source-release')}`,
  };
}

function assertRelativePath(root, value, label) {
  assert.equal(typeof value, 'string', `${label} must be a string`);
  assert.notEqual(value.length, 0, `${label} must not be empty`);
  assert.equal(isAbsolute(value), false, `${label} must be relative`);
  const resolved = resolve(root, value);
  const outside = relative(root, resolved);
  assert.equal(outside === '' || (!outside.startsWith('..') && !isAbsolute(outside)), true, `${label} escapes bundle root`);
  return resolved;
}

function verifyBundle({ root, sourceRelease }) {
  const builtinRoot = join(root, 'instruments', 'builtin');
  const manifestPath = join(builtinRoot, 'manifest.json');
  assert.equal(existsSync(manifestPath), true, `missing ${manifestPath}`);
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  assert.equal(manifest.sourceRelease, sourceRelease);
  assert.equal(Array.isArray(manifest.presets), true);
  assert.equal(manifest.presets.length > 0, true);

  const ids = manifest.presets.map((preset) => preset.id);
  assert.deepEqual(ids, [...ids].sort());
  assert.equal(new Set(ids).size, ids.length);
  for (const preset of manifest.presets) {
    assert.equal(typeof preset.id, 'string');
    assert.notEqual(preset.id.trim(), '');
    assert.equal(typeof preset.name, 'string');
    assert.notEqual(preset.name.trim(), '');
    assert.equal(preset.description === null || typeof preset.description === 'string', true);
    const definitionPath = assertRelativePath(builtinRoot, preset.definitionPath, 'definitionPath');
    const resourceBasePath = assertRelativePath(builtinRoot, preset.resourceBasePath, 'resourceBasePath');
    assert.equal(statSync(definitionPath).isFile(), true);
    assert.equal(statSync(resourceBasePath).isDirectory(), true);
  }
}

const isMainModule =
  process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (isMainModule) {
  try {
    verifyBundle(parseOptions(process.argv.slice(2)));
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
