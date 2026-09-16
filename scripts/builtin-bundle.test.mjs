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
    const sourceMetadata = JSON.parse(
      readFileSync(join(sourceRoot, 'presets', '01-clean-sub-bass', 'definition.json'), 'utf8'),
    ).metadata;
    const entry = manifest.presets[0];
    assert.equal(manifest.presets.length, 60);
    assert.equal(entry.id, '01-clean-sub-bass');
    assert.equal(entry.name, sourceMetadata.name);
    assert.equal(entry.author, sourceMetadata.author);
    assert.equal(entry.description, sourceMetadata.description);
    assert.equal(entry.category, sourceMetadata.category);
    assert.deepEqual(entry.tags, sourceMetadata.tags);
    assert.deepEqual(entry.recommendedRange, {
      minMidi: sourceMetadata.recommended_range.min_midi,
      maxMidi: sourceMetadata.recommended_range.max_midi,
    });
    assert.deepEqual(entry.preview, {
      tempoBpm: sourceMetadata.preview.tempo_bpm,
      ticksPerBeat: sourceMetadata.preview.ticks_per_beat,
      timeSignature: {
        numerator: sourceMetadata.preview.time_signature.numerator,
        denominator: sourceMetadata.preview.time_signature.denominator,
      },
      lengthTicks: sourceMetadata.preview.length_ticks,
      notes: sourceMetadata.preview.notes.map((note) => ({
        tick: note.tick,
        durationTicks: note.duration_ticks,
        note: note.note,
        velocity: note.velocity,
      })),
    });
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
