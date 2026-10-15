import assert from 'node:assert/strict';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { stageBuiltinBundle } from './stage-builtin-bundle.mjs';

const sourceRoot = fileURLToPath(new URL('..', import.meta.url));
const sourceRelease = readFileSync(join(sourceRoot, 'Cargo.toml'), 'utf8').match(
  /^version\s*=\s*"([^"]+)"/m,
)?.[1];
if (!sourceRelease) throw new Error('Cargo.toml package version is missing.');

const sonalloyBin = process.env.SONALLOY_BIN;
if (!sonalloyBin) throw new Error('Set SONALLOY_BIN to the absolute repository CLI path.');

const licenseFiles = ['THIRD_PARTY_NOTICES.md', 'LICENSE-MIT', 'LICENSE-APACHE'];

function sourcePresetIds() {
  return readdirSync(join(sourceRoot, 'presets'), { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && /^[A-Z]+$/.test(entry.name))
    .flatMap((category) =>
      readdirSync(join(sourceRoot, 'presets', category.name), { withFileTypes: true })
        .filter((entry) => entry.isDirectory() && /^\d{3}-/.test(entry.name))
        .map((entry) => `${category.name}-${entry.name.slice(0, 3)}`),
    )
    .sort();
}

function invalidSourceRoot(prefix) {
  const root = mkdtempSync(join(tmpdir(), prefix));
  for (const file of licenseFiles) {
    writeFileSync(join(root, file), 'test\n');
  }
  writeFileSync(join(root, 'Cargo.toml'), '[package]\nversion = "1.0.0"\n');
  return root;
}

test('staged built-in bundle contains source definitions and required resources', () => {
  const temporaryRoot = mkdtempSync(join(tmpdir(), 'sonalloy-bundle-'));
  try {
    const destination = join(temporaryRoot, 'bundle');
    stageBuiltinBundle({ sonalloyBin, destination, sourceRoot });
    const manifest = JSON.parse(
      readFileSync(join(destination, 'instruments', 'builtin', 'manifest.json'), 'utf8'),
    );
    assert.equal(manifest.sourceRelease, `v${sourceRelease}`);

    const presetIds = manifest.presets.map((preset) => preset.id);
    assert.deepEqual([...presetIds].sort(), sourcePresetIds());
    assert.equal(new Set(presetIds).size, presetIds.length, 'manifest has no duplicate IDs');

    const entry = manifest.presets.find((preset) => preset.id === 'BASS-001');
    assert.ok(entry, 'representative built-in preset is listed');
    const sourceDefinition = JSON.parse(
      readFileSync(
        join(sourceRoot, 'presets', ...entry.resourceBasePath.split('/'), 'definition.json'),
        'utf8',
      ),
    );
    const stagedDefinition = JSON.parse(
      readFileSync(join(destination, 'instruments', 'builtin', entry.definitionPath), 'utf8'),
    );
    assert.deepEqual(stagedDefinition, sourceDefinition);
    assert.equal(entry.definitionPath, `${entry.resourceBasePath}/definition.json`);
    assert.equal(entry.resourceBasePath, 'BASS/001-clean-sub-bass');

    for (const preset of manifest.presets) {
      assert.ok(preset.name, `${preset.id} keeps a name`);
      assert.ok(preset.category, `${preset.id} keeps a category`);
      assert.ok(Array.isArray(preset.tags) && preset.tags.length > 0, `${preset.id} keeps tags`);
      assert.ok(
        Number.isInteger(preset.recommendedRange?.minMidi) &&
          Number.isInteger(preset.recommendedRange?.maxMidi),
        `${preset.id} keeps a recommended range`,
      );
      assert.ok(
        Number.isFinite(preset.preview?.tempoBpm) &&
          Number.isInteger(preset.preview?.lengthTicks) &&
          preset.preview.notes.length > 0,
        `${preset.id} keeps a preview`,
      );
      assert.equal(
        existsSync(join(destination, 'instruments', 'builtin', preset.definitionPath)),
        true,
        `${preset.id} definition is staged`,
      );
    }

    for (const file of licenseFiles) {
      assert.equal(existsSync(join(destination, file)), true, file);
    }
    assert.equal(
      existsSync(join(destination, 'instruments', 'builtin', 'assets')),
      true,
      'built-in resources are staged',
    );
  } finally {
    rmSync(temporaryRoot, { force: true, recursive: true });
  }
});

test('staging rejects a preset with missing required metadata', () => {
  const temporaryRoot = invalidSourceRoot('sonalloy-bundle-invalid-');
  try {
    mkdirSync(join(temporaryRoot, 'presets', 'BASS', '001-broken'), { recursive: true });
    writeFileSync(
      join(temporaryRoot, 'presets', 'BASS', '001-broken', 'definition.json'),
      JSON.stringify({ metadata: { name: 'Broken' } }),
    );

    assert.throws(
      () =>
        stageBuiltinBundle({
          sonalloyBin,
          destination: join(temporaryRoot, 'bundle'),
          sourceRoot: temporaryRoot,
          sourceRelease: '1.0.0',
        }),
      /BASS-001.*metadata\.category/,
    );
    assert.equal(existsSync(join(temporaryRoot, 'bundle')), false);
  } finally {
    rmSync(temporaryRoot, { force: true, recursive: true });
  }
});

test('staging rejects an unexpected preset directory instead of omitting it', () => {
  const temporaryRoot = invalidSourceRoot('sonalloy-bundle-layout-');
  try {
    mkdirSync(join(temporaryRoot, 'presets', 'BASS', '12-broken'), { recursive: true });

    assert.throws(
      () =>
        stageBuiltinBundle({
          sonalloyBin,
          destination: join(temporaryRoot, 'bundle'),
          sourceRoot: temporaryRoot,
          sourceRelease: '1.0.0',
        }),
      /BASS\/12-broken.*NNN-kebab-case-name/,
    );
    assert.equal(existsSync(join(temporaryRoot, 'bundle')), false);
  } finally {
    rmSync(temporaryRoot, { force: true, recursive: true });
  }
});

test('staging rejects duplicate preset IDs', () => {
  const temporaryRoot = invalidSourceRoot('sonalloy-bundle-duplicate-');
  try {
    for (const directory of ['001-clean-sub-bass', '001-other-bass']) {
      mkdirSync(join(temporaryRoot, 'presets', 'BASS', directory), { recursive: true });
      writeFileSync(
        join(temporaryRoot, 'presets', 'BASS', directory, 'definition.json'),
        readFileSync(
          join(sourceRoot, 'presets', 'BASS', '001-clean-sub-bass', 'definition.json'),
        ),
      );
    }

    assert.throws(
      () =>
        stageBuiltinBundle({
          sonalloyBin,
          destination: join(temporaryRoot, 'bundle'),
          sourceRoot: temporaryRoot,
          sourceRelease: '1.0.0',
        }),
      /Duplicate built-in preset ID 'BASS-001'/,
    );
    assert.equal(existsSync(join(temporaryRoot, 'bundle')), false);
  } finally {
    rmSync(temporaryRoot, { force: true, recursive: true });
  }
});
