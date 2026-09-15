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
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const licenseFiles = ['THIRD_PARTY_NOTICES.md', 'LICENSE-MIT', 'LICENSE-APACHE'];
const usage =
  'Usage: node scripts/stage-builtin-bundle.mjs --destination <path> [--source-root <path>] [--source-release <tag>]';

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

function readPreset(sourceRoot, presetId) {
  const definitionPath = join(sourceRoot, 'presets', presetId, 'definition.json');
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
  const name = typeof metadata?.name === 'string' ? metadata.name.trim() : '';
  if (!name) throw new Error(`Sonalloy preset '${presetId}' has no metadata.name.`);
  const description =
    typeof metadata.description === 'string' && metadata.description.trim().length > 0
      ? metadata.description.trim()
      : null;
  const referencePitch =
    typeof metadata.reference_pitch === 'string' && metadata.reference_pitch.trim().length > 0
      ? metadata.reference_pitch.trim()
      : null;

  return {
    definitionPath,
    entry: {
      id: presetId,
      name,
      description,
      ...(referencePitch ? { referencePitch } : {}),
      definitionPath: `${presetId}/definition.json`,
      resourceBasePath: presetId,
    },
  };
}

function collectPresets(sourceRoot) {
  const presetsRoot = join(sourceRoot, 'presets');
  if (!existsSync(presetsRoot)) throw new Error(`Sonalloy presets directory is missing: ${presetsRoot}`);

  return readdirSync(presetsRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name !== 'assets')
    .map((entry) => entry.name)
    .sort()
    .map((presetId) => readPreset(sourceRoot, presetId));
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

export function stageBuiltinBundle({ destination, sourceRoot, sourceRelease }) {
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
    const presets = collectPresets(sourceRootPath);
    mkdirSync(stagedBuiltinRoot, { recursive: true });

    for (const { definitionPath, entry } of presets) {
      const presetDestination = join(stagedBuiltinRoot, entry.id);
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
    if (!['--destination', '--source-root', '--source-release'].includes(option) || !value) {
      throw new Error(usage);
    }
    if (options.has(option)) throw new Error(`Duplicate option: ${option}`);
    options.set(option, value);
  }
  const destination = options.get('--destination');
  if (!destination) throw new Error(usage);
  return {
    destination,
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
