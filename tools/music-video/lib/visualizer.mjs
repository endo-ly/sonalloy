import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { bands, levels, samples, trimAudio } from "./audio.mjs";
import { writeGallery } from "./gallery.mjs";
import { runRemotion, toolDirectory as root } from "./remotion.mjs";
import { prepareTracks } from "./tracks.mjs";
const presets = JSON.parse(
  readFileSync(path.join(root, "src/visualizers/presets.json"), "utf8"),
);

export function renderVisualizer(
  command,
  filename,
  selection = "all",
  stillTime = "12",
) {
  if (!["prepare", "studio", "still", "render"].includes(command) || !filename)
    throw new Error(
      "usage: node scripts/render.mjs <command> <config.json> [variant|all] [still-seconds]",
    );

  const configPath = path.resolve(filename);
  const config = JSON.parse(readFileSync(configPath, "utf8"));
  const {
    id,
    audio,
    durationSeconds = 30,
    startSeconds = 0,
    fadeOutSeconds = 0.5,
    fps = 30,
    width = 1080,
    height = 1920,
  } = config;
  if (typeof id !== "string" || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(id))
    throw new Error("id must use lowercase letters, digits and hyphens");
  if (typeof audio !== "string" || !audio) throw new Error("audio is required");
  for (const [key, value] of Object.entries({
    durationSeconds,
    fps,
    width,
    height,
  }))
    if (!Number.isFinite(value) || value <= 0)
      throw new Error(`${key} must be positive`);
  if (![width, height].every((value) => Number.isInteger(value) && value % 2 === 0))
    throw new Error("width and height must be even integers");
  if (
    !Number.isFinite(startSeconds) ||
    startSeconds < 0 ||
    !Number.isFinite(fadeOutSeconds) ||
    fadeOutSeconds < 0 ||
    fadeOutSeconds > durationSeconds
  )
    throw new Error("invalid clip range or fade duration");

  const variants = selection === "all" ? Object.keys(presets) : [selection];
  if (variants.some((variant) => !Object.hasOwn(presets, variant)))
    throw new Error(`unknown variant: ${selection}`);
  if (command === "studio" && variants.length !== 1)
    throw new Error("select one variant for studio");

  const seconds = Number(stillTime);
  if (
    command === "still" &&
    (!Number.isFinite(seconds) || seconds < 0 || seconds >= durationSeconds)
  )
    throw new Error("still time must fall inside the clip");

  const publicDir = path.join(root, "public", "visualizers", id);
  const outputDir = path.join(root, "out", "visualizers", id);
  mkdirSync(publicDir, { recursive: true });
  mkdirSync(outputDir, { recursive: true });

  const clip = { startSeconds, durationSeconds, fadeOutSeconds };
  const wav = path.join(publicDir, "mix.wav");
  trimAudio(path.resolve(path.dirname(configPath), audio), wav, clip);
  const pcm = samples(wav, { ...clip, startSeconds: 0 });
  const frames = Math.ceil(durationSeconds * fps);
  if (
    !Array.isArray(config.tracks) ||
    config.tracks.length < 1 ||
    config.tracks.length > 9
  )
    throw new Error("provide between one and nine tracks");

  const tracks = prepareTracks(
    config.tracks,
    path.dirname(configPath),
    clip,
    frames,
    fps,
    { includeScopes: true },
  );
  const melody = tracks.filter((track) => track.kind === "melody");
  const percussion = tracks.filter((track) => track.kind === "percussion");
  if (
    melody.length < 1 ||
    melody.length > 5 ||
    percussion.length > 4 ||
    melody.length + percussion.length !== tracks.length
  )
    throw new Error("provide 1–5 melody tracks and 0–4 percussion tracks");
  if (melody.some((track) => track.notes.length === 0))
    throw new Error("melody tracks need notes within the selected clip");

  writeFileSync(
    path.join(publicDir, "scene.json"),
    JSON.stringify({
      title: config.title ?? id,
      audio: `visualizers/${id}/mix.wav`,
      frames,
      fps,
      width,
      height,
      bands: bands(pcm, frames, fps),
      energy: levels(pcm, frames, fps),
      tracks,
      sections: config.sections ?? [],
    }),
  );

  for (const variant of variants) {
    const propsPath = path.join(outputDir, `${variant}.props.json`);
    writeFileSync(
      propsPath,
      JSON.stringify(
        {
          scenePath: `visualizers/${id}/scene.json`,
          variant,
        },
        null,
        2,
      ),
    );
    if (command === "prepare") continue;

    const args = [command, "src/index.tsx"];
    if (command !== "studio")
      args.push(
        "Visualizer",
        path.join(
          outputDir,
          `${variant}.${command === "still" ? `at-${seconds}s.png` : "mp4"}`,
        ),
      );
    args.push(`--props=${propsPath}`);
    if (command === "still") args.push(`--frame=${Math.floor(seconds * fps)}`);
    if (command === "render")
      args.push(
        "--codec=h264",
        "--crf=18",
        "--pixel-format=yuv420p",
        "--audio-bitrate=256k",
        "--concurrency=4",
      );
    runRemotion(args, propsPath);
  }

  if (command === "render")
    writeGallery(
      outputDir,
      id,
      Object.keys(presets).filter((name) =>
        existsSync(path.join(outputDir, `${name}.mp4`)),
      ),
      durationSeconds,
    );
  console.log(`Output: ${outputDir}`);
}
