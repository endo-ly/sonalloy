import assert from 'node:assert/strict';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const usage = 'Usage: node scripts/verify-builtin-bundle.mjs --root <path> --source-release <tag>';
const BUILTIN_TAG_VOCABULARY = new Set([
  'Warm',
  'Bright',
  'Dark',
  'Clean',
  'Noisy',
  'Metallic',
  'Glassy',
  'Soft',
  'Aggressive',
  'Punchy',
  'Wide',
  'Deep',
  'Analog',
  'Digital',
  'FM',
  'Wavetable',
  'Wavefold',
  'Additive',
  'Formant',
  'Granular',
  'Physical',
  'Spectral',
  'Noise',
  'Mono',
  'Polyphonic',
  'Motion',
  'Rhythmic',
  'Sustained',
  'Plucky',
  'Percussive',
  'Evolving',
  'Gated',
  'Random',
  'Sub',
  'Acid',
  'Reese',
  'Supersaw',
  'Drone',
  'Chord',
  'Bell',
  'Kick',
  'Snare',
  'Clap',
  'Hihat',
  'Crash',
  'Tom',
  'Rim',
  'Shaker',
  'Riser',
  'Impact',
  'Sequence',
  'Texture',
]);

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

function assertRecord(value, label) {
  assert.equal(typeof value, 'object', `${label} must be an object`);
  assert.notEqual(value, null, `${label} must not be null`);
  assert.equal(Array.isArray(value), false, `${label} must be an object`);
}

function assertSafeInteger(value, label) {
  assert.equal(Number.isSafeInteger(value), true, `${label} must be a safe integer`);
}

function asciiLower(value) {
  return value.replace(/[A-Z]/g, (character) => character.toLowerCase());
}

function expectedCategory(id) {
  const number = Number.parseInt(id.slice(0, 2), 10);
  if (number <= 9) return 'Bass';
  if (number <= 16) return 'Lead';
  if (number <= 24) return 'Pad';
  if (number <= 26) return 'Keys';
  if (number <= 29) return 'Poly';
  if (number <= 31) return 'Stab';
  if (number <= 34) return 'Pluck';
  if (number <= 37) return 'Mallet';
  if (number <= 47) return 'Drums';
  if (number <= 49) return 'Percussion';
  if (number <= 54) return 'Sequence';
  return 'FX';
}

function assertMetadata(preset) {
  assert.equal(preset.author === null || typeof preset.author === 'string', true);
  assert.equal(preset.description === null || typeof preset.description === 'string', true);
  assert.equal(typeof preset.category, 'string');
  assert.notEqual(preset.category.trim(), '');
  assert.equal(preset.category, preset.category.trim());
  assert.equal([...preset.category].length <= 64, true);
  assert.equal([...preset.category].some((character) => /\p{Cc}/u.test(character)), false);

  assert.equal(Array.isArray(preset.tags), true);
  assert.equal(preset.tags.length >= 2 && preset.tags.length <= 5, true);
  const normalizedTags = new Set();
  for (const [index, tag] of preset.tags.entries()) {
    assert.equal(typeof tag, 'string', `tags[${index}] must be a string`);
    assert.notEqual(tag.trim(), '', `tags[${index}] must not be empty`);
    assert.equal(tag, tag.trim(), `tags[${index}] must not have outer whitespace`);
    assert.equal([...tag].length <= 32, true, `tags[${index}] is too long`);
    assert.equal([...tag].some((character) => /\p{Cc}/u.test(character)), false);
    assert.equal(
      BUILTIN_TAG_VOCABULARY.has(tag),
      true,
      `${preset.id} tags[${index}] is not in the Built-in Tag vocabulary`,
    );
    const normalizedTag = asciiLower(tag);
    assert.equal(normalizedTags.has(normalizedTag), false, `tags[${index}] is duplicated`);
    normalizedTags.add(normalizedTag);
  }

  assertRecord(preset.recommendedRange, 'recommendedRange');
  assertSafeInteger(preset.recommendedRange.minMidi, 'recommendedRange.minMidi');
  assertSafeInteger(preset.recommendedRange.maxMidi, 'recommendedRange.maxMidi');
  assert.equal(preset.recommendedRange.minMidi >= 0 && preset.recommendedRange.minMidi <= 127, true);
  assert.equal(preset.recommendedRange.maxMidi >= 0 && preset.recommendedRange.maxMidi <= 127, true);
  assert.equal(preset.recommendedRange.minMidi <= preset.recommendedRange.maxMidi, true);

  assertRecord(preset.preview, 'preview');
  assert.equal(Number.isFinite(preset.preview.tempoBpm), true);
  assert.equal(preset.preview.tempoBpm >= 30 && preset.preview.tempoBpm <= 300, true);
  assertSafeInteger(preset.preview.ticksPerBeat, 'preview.ticksPerBeat');
  assert.equal(preset.preview.ticksPerBeat >= 1 && preset.preview.ticksPerBeat <= 32767, true);
  assertRecord(preset.preview.timeSignature, 'preview.timeSignature');
  assertSafeInteger(preset.preview.timeSignature.numerator, 'preview.timeSignature.numerator');
  assertSafeInteger(preset.preview.timeSignature.denominator, 'preview.timeSignature.denominator');
  assert.equal(
    preset.preview.timeSignature.numerator >= 1 && preset.preview.timeSignature.numerator <= 32,
    true,
  );
  assert.equal(
    preset.preview.timeSignature.denominator >= 1 &&
      preset.preview.timeSignature.denominator <= 128 &&
      (preset.preview.timeSignature.denominator & (preset.preview.timeSignature.denominator - 1)) === 0,
    true,
  );
  assertSafeInteger(preset.preview.lengthTicks, 'preview.lengthTicks');
  assert.equal(preset.preview.lengthTicks > 0, true);
  assert.equal(Array.isArray(preset.preview.notes), true);
  assert.equal(preset.preview.notes.length >= 1 && preset.preview.notes.length <= 32, true);

  let previousTick = null;
  for (const [index, note] of preset.preview.notes.entries()) {
    assertRecord(note, `preview.notes[${index}]`);
    assertSafeInteger(note.tick, `preview.notes[${index}].tick`);
    assertSafeInteger(note.durationTicks, `preview.notes[${index}].durationTicks`);
    assertSafeInteger(note.note, `preview.notes[${index}].note`);
    assertSafeInteger(note.velocity, `preview.notes[${index}].velocity`);
    assert.equal(note.note >= 0 && note.note <= 127, true);
    assert.equal(note.velocity >= 1 && note.velocity <= 127, true);
    assert.equal(note.tick < preset.preview.lengthTicks, true);
    assert.equal(note.durationTicks > 0, true);
    assert.equal(note.durationTicks <= preset.preview.lengthTicks - note.tick, true);
    if (previousTick !== null) assert.equal(note.tick >= previousTick, true);
    previousTick = note.tick;
    assert.equal(
      note.note >= preset.recommendedRange.minMidi && note.note <= preset.recommendedRange.maxMidi,
      true,
    );
  }

  const previewSeconds =
    (preset.preview.lengthTicks / preset.preview.ticksPerBeat) * 60 / preset.preview.tempoBpm;
  assert.equal(Number.isFinite(previewSeconds), true);
  assert.equal(previewSeconds <= 10, true);
}

export function verifyBuiltinBundle({ root, sourceRelease }) {
  const builtinRoot = join(root, 'instruments', 'builtin');
  const manifestPath = join(builtinRoot, 'manifest.json');
  assert.equal(existsSync(manifestPath), true, `missing ${manifestPath}`);
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  assert.equal(manifest.sourceRelease, sourceRelease);
  assert.equal(Array.isArray(manifest.presets), true);
  assert.equal(manifest.presets.length, 60);

  const ids = manifest.presets.map((preset, index) => {
    assertRecord(preset, `manifest.presets[${index}]`);
    return preset.id;
  });
  assert.deepEqual(ids, [...ids].sort());
  assert.equal(new Set(ids).size, ids.length);
  for (const preset of manifest.presets) {
    assert.equal(typeof preset.id, 'string');
    assert.notEqual(preset.id.trim(), '');
    assert.match(preset.id, /^(?:0[1-9]|[1-5][0-9]|60)-.+$/);
    assert.equal(typeof preset.name, 'string');
    assert.notEqual(preset.name.trim(), '');
    assert.equal(preset.definitionPath, `${preset.id}/definition.json`);
    assert.equal(preset.resourceBasePath, preset.id);
    assert.equal(preset.category, expectedCategory(preset.id));
    assertMetadata(preset);
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
    verifyBuiltinBundle(parseOptions(process.argv.slice(2)));
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
