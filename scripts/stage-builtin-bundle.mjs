import { spawnSync } from 'node:child_process';
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { randomUUID } from 'node:crypto';
import { basename, dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const licenseFiles = ['THIRD_PARTY_NOTICES.md', 'LICENSE-MIT', 'LICENSE-APACHE'];
const usage =
  'Usage: node scripts/stage-builtin-bundle.mjs --destination <path> --sonalloy-bin <absolute-path> [--source-root <path>] [--source-release <tag>]';

function normalizeReleaseTag(value) {
  if (!value) throw new Error('A Sonalloy source release is required.');
  return value.startsWith('v') ? value : `v${value}`;
}

function readSourceRelease(sourceRoot) {
  const cargoToml = readFileSync(join(sourceRoot, 'Cargo.toml'), 'utf8');
  const version = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) throw new Error(`Sonalloy version could not be read from ${join(sourceRoot, 'Cargo.toml')}.`);
  return normalizeReleaseTag(version);
}

const presetCategories = new Set([
  'BASS',
  'LEAD',
  'PAD',
  'KEYS',
  'PLUCK',
  'MALLET',
  'DRUM',
  'SEQ',
  'FX',
]);
const presetCategoryLabels = {
  BASS: 'Bass',
  LEAD: 'Lead',
  PAD: 'Pad',
  KEYS: 'Keys',
  PLUCK: 'Pluck',
  MALLET: 'Mallet',
  DRUM: 'Drum',
  SEQ: 'Sequence',
  FX: 'FX',
};
const ignoredPresetDirectories = new Set(['assets', 'common-scores']);
const presetDirectoryPattern = /^\d{3}-[a-z0-9]+(?:-[a-z0-9]+)*$/;

function manifestPath(value) {
  return value.split('\\').join('/');
}

function readPreset(sourceRoot, presetPath, presetId, categoryCode, sonalloyBin) {
  const definitionPath = join(sourceRoot, 'presets', ...presetPath.split('/'), 'definition.json');
  let definition;
  try {
    definition = JSON.parse(readFileSync(definitionPath, 'utf8'));
  } catch (error) {
    throw new Error(
      `Sonalloy preset '${presetId}' definition could not be read: ${
        error instanceof Error ? error.message : error
      }`,
    );
  }

  const metadata = definition?.metadata;
  if (!metadata || typeof metadata !== 'object' || Array.isArray(metadata)) {
    throw new Error(`Sonalloy preset '${presetId}' has no metadata object.`);
  }
  const name = typeof metadata?.name === 'string' ? metadata.name.trim() : '';
  if (!name) throw new Error(`Sonalloy preset '${presetId}' has no metadata.name.`);
  const author = readOptionalText(metadata.author, presetId, 'author');
  const description = readOptionalText(metadata.description, presetId, 'description');
  const category = readRequiredText(metadata.category, presetId, 'category');
  if (category !== presetCategoryLabels[categoryCode]) {
    throw new Error(
      `Sonalloy preset '${presetId}' metadata.category must be '${presetCategoryLabels[categoryCode]}'.`,
    );
  }
  const tags = readRequiredArray(metadata.tags, presetId, 'tags');
  const recommendedRange = readRequiredRecord(
    metadata.recommended_range,
    presetId,
    'recommended_range',
  );
  if (!Object.hasOwn(recommendedRange, 'min_midi') || !Object.hasOwn(recommendedRange, 'max_midi')) {
    throw new Error(`Sonalloy preset '${presetId}' metadata.recommended_range is incomplete.`);
  }
  const result = spawnSync(sonalloyBin, ['score', 'inspect', join(sourceRoot, 'presets', presetPath, 'preview.score.yaml'), '--preset-root', join(sourceRoot, 'presets'), '--preview-json'], { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error(`Sonalloy preset '${presetId}' preview compilation failed: ${result.error?.message ?? result.stderr}`);
  const preview = JSON.parse(result.stdout);

  return {
    definitionPath,
    entry: {
      id: presetId,
      name,
      author,
      description,
      category,
      tags,
      recommendedRange: {
        minMidi: recommendedRange.min_midi,
        maxMidi: recommendedRange.max_midi,
      },
      preview,
      definitionPath: `${manifestPath(presetPath)}/definition.json`,
      resourceBasePath: manifestPath(presetPath),
    },
  };
}

function readOptionalText(value, presetId, field) {
  if (value === undefined || value === null) return null;
  if (typeof value !== 'string') {
    throw new Error(`Sonalloy preset '${presetId}' metadata.${field} must be a string or null.`);
  }
  return value.trim().length > 0 ? value.trim() : null;
}

function readRequiredText(value, presetId, field) {
  if (typeof value !== 'string' || value.trim().length === 0) {
    throw new Error(`Sonalloy preset '${presetId}' has no metadata.${field}.`);
  }
  if (value !== value.trim()) {
    throw new Error(`Sonalloy preset '${presetId}' metadata.${field} must not have outer whitespace.`);
  }
  return value;
}

function readRequiredArray(value, presetId, field) {
  if (!Array.isArray(value)) {
    throw new Error(`Sonalloy preset '${presetId}' has no metadata.${field} array.`);
  }
  return value;
}

function readRequiredRecord(value, presetId, field) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`Sonalloy preset '${presetId}' has no metadata.${field} object.`);
  }
  return value;
}

function collectPresets(sourceRoot, sonalloyBin) {
  const presetsRoot = join(sourceRoot, 'presets');
  if (!existsSync(presetsRoot)) throw new Error(`Sonalloy presets directory is missing: ${presetsRoot}`);

  const presets = [];
  const presetIds = new Set();
  const rootEntries = readdirSync(presetsRoot, { withFileTypes: true });
  for (const entry of rootEntries) {
    if (entry.isDirectory() && !ignoredPresetDirectories.has(entry.name) && !presetCategories.has(entry.name)) {
      throw new Error(
        `Unexpected preset category directory '${entry.name}'. Expected one of: ${[...presetCategories].join(', ')}.`,
      );
    }
  }

  for (const category of rootEntries
    .filter((entry) => entry.isDirectory() && presetCategories.has(entry.name))
    .sort((left, right) => left.name.localeCompare(right.name))) {
    const categoryRoot = join(presetsRoot, category.name);
    for (const preset of readdirSync(categoryRoot, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .sort((left, right) => left.name.localeCompare(right.name))) {
      if (!presetDirectoryPattern.test(preset.name)) {
        throw new Error(
          `Preset directory '${category.name}/${preset.name}' must match NNN-kebab-case-name.`,
        );
      }
      const presetPath = `${category.name}/${preset.name}`;
      const presetId = `${category.name}-${preset.name.slice(0, 3)}`;
      if (presetIds.has(presetId)) {
        throw new Error(`Duplicate built-in preset ID '${presetId}'.`);
      }
      presetIds.add(presetId);
      presets.push({ presetPath, presetId, categoryCode: category.name });
    }
  }
  return presets.map(({ presetPath, presetId, categoryCode }) => readPreset(sourceRoot, presetPath, presetId, categoryCode, sonalloyBin));
}

function replaceDirectory(destination, stagedDestination) {
  const backupDestination = join(
    dirname(destination),
    `.${basename(destination)}.backup-${randomUUID()}`,
  );
  let movedExistingDestination = false;

  try {
    if (existsSync(destination)) {
      renameSync(destination, backupDestination);
      movedExistingDestination = true;
    }
    renameSync(stagedDestination, destination);
    if (movedExistingDestination) rmSync(backupDestination, { force: true, recursive: true });
  } catch (error) {
    if (movedExistingDestination && !existsSync(destination) && existsSync(backupDestination)) {
      renameSync(backupDestination, destination);
    }
    throw error;
  }
}

export function stageBuiltinBundle({ destination, sourceRoot, sourceRelease, sonalloyBin }) {
  if (!sonalloyBin || !isAbsolute(sonalloyBin) || !existsSync(sonalloyBin)) throw new Error('An existing absolute --sonalloy-bin path is required.');
  if (!destination) throw new Error('A destination for built-in resources is required.');
  const sourceRootPath = resolve(sourceRoot ?? resolve(dirname(fileURLToPath(import.meta.url)), '..'));
  if (!existsSync(sourceRootPath)) throw new Error(`Sonalloy source directory is missing: ${sourceRootPath}`);

  const destinationRoot = resolve(destination);
  const stagedRoot = join(
    dirname(destinationRoot),
    `.${basename(destinationRoot)}.tmp-${randomUUID()}`,
  );
  const releaseTag = normalizeReleaseTag(sourceRelease ?? readSourceRelease(sourceRootPath));

  try {
    mkdirSync(dirname(destinationRoot), { recursive: true });
    const stagedBuiltinRoot = join(stagedRoot, 'instruments', 'builtin');
    const presets = collectPresets(sourceRootPath, sonalloyBin);
    mkdirSync(stagedBuiltinRoot, { recursive: true });

    for (const { definitionPath, entry } of presets) {
      const presetDestination = join(stagedBuiltinRoot, entry.resourceBasePath);
      mkdirSync(presetDestination, { recursive: true });
      copyFileSync(definitionPath, join(presetDestination, 'definition.json'));
    }

    const sourceAssets = join(sourceRootPath, 'presets', 'assets');
    if (existsSync(sourceAssets)) cpSync(sourceAssets, join(stagedBuiltinRoot, 'assets'), { recursive: true });

    writeFileSync(
      join(stagedBuiltinRoot, 'manifest.json'),
      `${JSON.stringify(
        {
          sourceRelease: releaseTag,
          presets: presets.map(({ entry }) => entry),
        },
        null,
        2,
      )}\n`,
    );

    for (const licenseFile of licenseFiles) {
      const source = join(sourceRootPath, licenseFile);
      if (!existsSync(source)) throw new Error(`Sonalloy source is missing ${licenseFile}: ${source}`);
      mkdirSync(stagedRoot, { recursive: true });
      copyFileSync(source, join(stagedRoot, licenseFile));
    }

    replaceDirectory(destinationRoot, stagedRoot);
  } finally {
    if (existsSync(stagedRoot)) rmSync(stagedRoot, { force: true, recursive: true });
  }
}

function parseOptions(args) {
  if (args.length < 2 || args.length % 2 !== 0) throw new Error(usage);
  const options = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const option = args[index];
    const value = args[index + 1];
    if (!['--destination', '--source-root', '--source-release', '--sonalloy-bin'].includes(option) || !value) {
      throw new Error(usage);
    }
    if (options.has(option)) throw new Error(`Duplicate option: ${option}`);
    options.set(option, value);
  }
  const destination = options.get('--destination');
  if (!destination) throw new Error(usage);
  return {
    destination,
    sonalloyBin: options.get('--sonalloy-bin'),
    sourceRoot: options.get('--source-root'),
    sourceRelease: options.get('--source-release'),
  };
}

const isMainModule =
  process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (isMainModule) {
  try {
    const options = parseOptions(process.argv.slice(2));
    stageBuiltinBundle(options);
    console.log(`Built-in bundle staged at ${resolve(options.destination)}`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
