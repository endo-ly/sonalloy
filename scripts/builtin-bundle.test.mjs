import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { stageBuiltinBundle } from './stage-builtin-bundle.mjs';
import { verifyBuiltinBundle } from './verify-builtin-bundle.mjs';

const sourceRoot = fileURLToPath(new URL('..', import.meta.url));
const sourceVersion = readFileSync(join(sourceRoot, 'Cargo.toml'), 'utf8').match(
  /^version\s*=\s*"([^"]+)"/m,
)?.[1];
if (!sourceVersion) throw new Error('Cargo.toml package version is missing.');
const sourceRelease = `v${sourceVersion}`;

function normalizeOptionalText(value) {
  if (value === undefined || value === null) return null;
  return value.trim().length > 0 ? value.trim() : null;
}

function expectedManifestEntry(presetId, metadata) {
  return {
    id: presetId,
    name: metadata.name.trim(),
    author: normalizeOptionalText(metadata.author),
    description: normalizeOptionalText(metadata.description),
    category: metadata.category,
    tags: metadata.tags,
    recommendedRange: {
      minMidi: metadata.recommended_range.min_midi,
      maxMidi: metadata.recommended_range.max_midi,
    },
    preview: {
      tempoBpm: metadata.preview.tempo_bpm,
      ticksPerBeat: metadata.preview.ticks_per_beat,
      timeSignature: {
        numerator: metadata.preview.time_signature.numerator,
        denominator: metadata.preview.time_signature.denominator,
      },
      lengthTicks: metadata.preview.length_ticks,
      notes: metadata.preview.notes.map((note) => ({
        tick: note.tick,
        durationTicks: note.duration_ticks,
        note: note.note,
        velocity: note.velocity,
      })),
    },
    definitionPath: `${presetId}/definition.json`,
    resourceBasePath: presetId,
  };
}

test('staged built-in bundle contains all metadata for every preset', () => {
  const temporaryRoot = mkdtempSync(join(tmpdir(), 'sonalloy-bundle-'));
  try {
    const destination = join(temporaryRoot, 'bundle');
    stageBuiltinBundle({
      destination,
      sourceRoot,
    });
    verifyBuiltinBundle({ root: destination, sourceRelease });

    const manifest = JSON.parse(
      readFileSync(join(destination, 'instruments', 'builtin', 'manifest.json'), 'utf8'),
    );
    assert.equal(manifest.presets.length, 60);
    for (const entry of manifest.presets) {
      const sourceDefinition = JSON.parse(
        readFileSync(join(sourceRoot, 'presets', entry.id, 'definition.json'), 'utf8'),
      );
      assert.deepEqual(entry, expectedManifestEntry(entry.id, sourceDefinition.metadata), entry.id);
    }
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
