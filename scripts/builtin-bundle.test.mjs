import assert from 'node:assert/strict';
import {
  copyFileSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
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
const sourceVersion = readFileSync(join(sourceRoot, 'Cargo.toml'), 'utf8').match(
  /^version\s*=\s*"([^"]+)"/m,
)?.[1];
if (!sourceVersion) throw new Error('Cargo.toml package version is missing.');
const sourceRelease = `v${sourceVersion}`;

test('staged built-in bundle contains source definitions and required resources', () => {
  const temporaryRoot = mkdtempSync(join(tmpdir(), 'sonalloy-bundle-'));
  try {
    const destination = join(temporaryRoot, 'bundle');
    stageBuiltinBundle({
      destination,
      sourceRoot,
    });
    const manifest = JSON.parse(
      readFileSync(join(destination, 'instruments', 'builtin', 'manifest.json'), 'utf8'),
    );
    assert.equal(manifest.sourceRelease, sourceRelease);
    const sourcePresetIds = readdirSync(join(sourceRoot, 'presets'), { withFileTypes: true })
      .filter((entry) => entry.isDirectory() && /^[A-Z]+$/.test(entry.name))
      .flatMap((category) =>
        readdirSync(join(sourceRoot, 'presets', category.name), { withFileTypes: true })
          .filter((entry) => entry.isDirectory() && /^\d{3}-/.test(entry.name))
          .map((entry) => `${category.name}-${entry.name.slice(0, 3)}`),
      )
      .sort();
    assert.deepEqual(
      manifest.presets.map((preset) => preset.id).sort(),
      sourcePresetIds,
      'manifest contains exactly the source preset directories',
    );

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
    assert.equal(entry.name, sourceDefinition.metadata.name);
    assert.equal(entry.author, sourceDefinition.metadata.author);
    assert.equal(entry.description, sourceDefinition.metadata.description);
    assert.equal(entry.category, sourceDefinition.metadata.category);
    assert.deepEqual(entry.tags, sourceDefinition.metadata.tags);
    assert.equal(
      entry.recommendedRange.minMidi,
      sourceDefinition.metadata.recommended_range.min_midi,
    );
    assert.equal(
      entry.recommendedRange.maxMidi,
      sourceDefinition.metadata.recommended_range.max_midi,
    );
    assert.equal(entry.preview.tempoBpm, sourceDefinition.metadata.preview.tempo_bpm);
    assert.equal(entry.preview.ticksPerBeat, sourceDefinition.metadata.preview.ticks_per_beat);
    assert.equal(
      entry.preview.timeSignature.numerator,
      sourceDefinition.metadata.preview.time_signature.numerator,
    );
    assert.equal(
      entry.preview.timeSignature.denominator,
      sourceDefinition.metadata.preview.time_signature.denominator,
    );
    assert.equal(entry.preview.lengthTicks, sourceDefinition.metadata.preview.length_ticks);
    assert.equal(entry.preview.notes.length, sourceDefinition.metadata.preview.notes.length);
    assert.equal(entry.preview.notes[0].tick, sourceDefinition.metadata.preview.notes[0].tick);
    assert.equal(
      entry.preview.notes[0].durationTicks,
      sourceDefinition.metadata.preview.notes[0].duration_ticks,
    );
    assert.equal(entry.preview.notes[0].note, sourceDefinition.metadata.preview.notes[0].note);
    assert.equal(
      entry.preview.notes[0].velocity,
      sourceDefinition.metadata.preview.notes[0].velocity,
    );
    assert.equal(entry.definitionPath, `${entry.resourceBasePath}/definition.json`);
    assert.equal(entry.resourceBasePath, 'BASS/001-clean-sub-bass');
    for (const file of ['THIRD_PARTY_NOTICES.md', 'LICENSE-MIT', 'LICENSE-APACHE']) {
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
  const temporaryRoot = mkdtempSync(join(tmpdir(), 'sonalloy-bundle-invalid-'));
  try {
    mkdirSync(join(temporaryRoot, 'presets', 'BASS', '001-broken'), { recursive: true });
    for (const file of ['THIRD_PARTY_NOTICES.md', 'LICENSE-MIT', 'LICENSE-APACHE']) {
      writeFileSync(join(temporaryRoot, file), 'test\n');
    }
    writeFileSync(join(temporaryRoot, 'Cargo.toml'), '[package]\nversion = "1.0.0"\n');
    writeFileSync(
      join(temporaryRoot, 'presets', 'BASS', '001-broken', 'definition.json'),
      JSON.stringify({ metadata: { name: 'Broken' } }),
    );

    assert.throws(
      () =>
        stageBuiltinBundle({
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
  const temporaryRoot = mkdtempSync(join(tmpdir(), 'sonalloy-bundle-layout-'));
  try {
    mkdirSync(join(temporaryRoot, 'presets', 'BASS', '12-broken'), { recursive: true });
    for (const file of ['THIRD_PARTY_NOTICES.md', 'LICENSE-MIT', 'LICENSE-APACHE']) {
      writeFileSync(join(temporaryRoot, file), 'test\n');
    }
    writeFileSync(join(temporaryRoot, 'Cargo.toml'), '[package]\nversion = "1.0.0"\n');

    assert.throws(
      () =>
        stageBuiltinBundle({
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
  const temporaryRoot = mkdtempSync(join(tmpdir(), 'sonalloy-bundle-duplicate-'));
  try {
    for (const directory of ['001-clean-sub-bass', '001-other-bass']) {
      mkdirSync(join(temporaryRoot, 'presets', 'BASS', directory), { recursive: true });
      copyFileSync(
        join(sourceRoot, 'presets', 'BASS', '001-clean-sub-bass', 'definition.json'),
        join(temporaryRoot, 'presets', 'BASS', directory, 'definition.json'),
      );
    }
    for (const file of ['THIRD_PARTY_NOTICES.md', 'LICENSE-MIT', 'LICENSE-APACHE']) {
      writeFileSync(join(temporaryRoot, file), 'test\n');
    }
    writeFileSync(join(temporaryRoot, 'Cargo.toml'), '[package]\nversion = "1.0.0"\n');

    assert.throws(
      () =>
        stageBuiltinBundle({
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
