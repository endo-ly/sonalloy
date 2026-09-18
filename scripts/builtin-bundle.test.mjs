import assert from 'node:assert/strict';
import {
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
      .filter((entry) => entry.isDirectory() && entry.name !== 'assets')
      .map((entry) => entry.name)
      .sort();
    assert.deepEqual(
      manifest.presets.map((preset) => preset.id).sort(),
      sourcePresetIds,
      'manifest contains exactly the source preset directories',
    );

    const entry = manifest.presets.find((preset) => preset.id === '01-clean-sub-bass');
    assert.ok(entry, 'representative built-in preset is listed');
    const sourceDefinition = JSON.parse(
      readFileSync(join(sourceRoot, 'presets', entry.id, 'definition.json'), 'utf8'),
    );
    const stagedDefinition = JSON.parse(
      readFileSync(join(destination, 'instruments', 'builtin', entry.definitionPath), 'utf8'),
    );
    assert.deepEqual(stagedDefinition, sourceDefinition);
    assert.deepEqual(
      {
        id: entry.id,
        name: entry.name,
        author: entry.author,
        description: entry.description,
        category: entry.category,
        tags: entry.tags,
        recommendedRange: entry.recommendedRange,
        preview: entry.preview,
        definitionPath: entry.definitionPath,
        resourceBasePath: entry.resourceBasePath,
      },
      {
        id: entry.id,
        name: sourceDefinition.metadata.name,
        author: sourceDefinition.metadata.author,
        description: sourceDefinition.metadata.description,
        category: sourceDefinition.metadata.category,
        tags: sourceDefinition.metadata.tags,
        recommendedRange: {
          minMidi: sourceDefinition.metadata.recommended_range.min_midi,
          maxMidi: sourceDefinition.metadata.recommended_range.max_midi,
        },
        preview: {
          tempoBpm: sourceDefinition.metadata.preview.tempo_bpm,
          ticksPerBeat: sourceDefinition.metadata.preview.ticks_per_beat,
          timeSignature: {
            numerator: sourceDefinition.metadata.preview.time_signature.numerator,
            denominator: sourceDefinition.metadata.preview.time_signature.denominator,
          },
          lengthTicks: sourceDefinition.metadata.preview.length_ticks,
          notes: sourceDefinition.metadata.preview.notes.map((note) => ({
            tick: note.tick,
            durationTicks: note.duration_ticks,
            note: note.note,
            velocity: note.velocity,
          })),
        },
        definitionPath: `${entry.id}/definition.json`,
        resourceBasePath: entry.id,
      },
    );
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
    mkdirSync(join(temporaryRoot, 'presets', '01-broken'), { recursive: true });
    for (const file of ['THIRD_PARTY_NOTICES.md', 'LICENSE-MIT', 'LICENSE-APACHE']) {
      writeFileSync(join(temporaryRoot, file), 'test\n');
    }
    writeFileSync(join(temporaryRoot, 'Cargo.toml'), '[package]\nversion = "1.0.0"\n');
    writeFileSync(
      join(temporaryRoot, 'presets', '01-broken', 'definition.json'),
      JSON.stringify({ metadata: { name: 'Broken' } }),
    );

    assert.throws(
      () =>
        stageBuiltinBundle({
          destination: join(temporaryRoot, 'bundle'),
          sourceRoot: temporaryRoot,
          sourceRelease: '1.0.0',
        }),
      /01-broken.*metadata\.category/,
    );
    assert.equal(existsSync(join(temporaryRoot, 'bundle')), false);
  } finally {
    rmSync(temporaryRoot, { force: true, recursive: true });
  }
});
