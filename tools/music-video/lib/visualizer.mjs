import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { bands, levels, samples, trimAudio } from "./audio.mjs";
import { writeGallery } from "./gallery.mjs";
import { runRemotion, toolDirectory } from "./remotion.mjs";
import { prepareTracks } from "./tracks.mjs";

export function renderVisualizer(
  project,
  command,
  selection = "all",
  stillTime = "12",
) {
  const { durationSeconds } = project.clip;
  const { width, height, variants: availableVariants } = project;
  if (!Number.isFinite(width) || !Number.isFinite(height))
    throw new Error("visualizer width and height are required");
  if (![width, height].every((value) => Number.isInteger(value) && value % 2 === 0))
    throw new Error("width and height must be even integers");
  const variants =
    selection === "all" ? availableVariants : [selection];
  if (!variants.length) throw new Error("visualizer variants are required");
  if (variants.some((variant) => !availableVariants.includes(variant)))
    throw new Error(`unknown variant: ${selection}`);
  if (command === "studio" && variants.length !== 1)
    throw new Error("select one variant for studio");

  const seconds = Number(stillTime);
  if (
    command === "still" &&
    (!Number.isFinite(seconds) || seconds < 0 || seconds >= durationSeconds)
  )
    throw new Error("still time must fall inside the clip");

  const publicDirectory = path.join(
    toolDirectory,
    "public",
    "visualizers",
    project.id,
  );
  const outputDirectory = project.outputDirectory;
  mkdirSync(publicDirectory, { recursive: true });
  mkdirSync(outputDirectory, { recursive: true });

  const wav = path.join(publicDirectory, "mix.wav");
  trimAudio(project.audio, wav, project.clip);
  const pcm = samples(wav, { ...project.clip, startSeconds: 0 });
  const frames = Math.ceil(durationSeconds * project.fps);
  const tracks = prepareTracks(
    project.tracks,
    project.directory,
    project.clip,
    frames,
    project.fps,
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
    path.join(publicDirectory, "scene.json"),
    JSON.stringify({
      title: project.title,
      audio: `visualizers/${project.id}/mix.wav`,
      frames,
      fps: project.fps,
      width,
      height,
      bands: bands(pcm, frames, project.fps),
      energy: levels(pcm, frames, project.fps),
      tracks,
      sections: project.sections,
    }),
  );

  for (const variant of variants) {
    const propsPath = path.join(outputDirectory, `${variant}.props.json`);
    writeFileSync(
      propsPath,
      JSON.stringify(
        {
          scenePath: `visualizers/${project.id}/scene.json`,
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
          outputDirectory,
          `${variant}.${command === "still" ? `at-${seconds}s.png` : "mp4"}`,
        ),
      );
    if (command === "still") args.push(`--frame=${Math.floor(seconds * project.fps)}`);
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
      outputDirectory,
      project.id,
      availableVariants.filter((variant) =>
        existsSync(path.join(outputDirectory, `${variant}.mp4`)),
      ),
      durationSeconds,
    );
  console.log(`Output: ${outputDirectory}`);
}
