import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { bands, levels, samples, trimAudio } from "./audio.mjs";
import { exportVideo } from "./export.mjs";
import { runRemotion, toolDirectory } from "./remotion.mjs";
import { prepareTracks } from "./tracks.mjs";

export function prepareInstrumentScore(project) {
  const { id, clip, frames, fps } = project;
  const publicDirectory = path.join(toolDirectory, "public", id);
  mkdirSync(publicDirectory, { recursive: true });
  mkdirSync(project.outputDirectory, { recursive: true });
  const tracks = prepareTracks(
    project.tracks,
    project.directory,
    clip,
    frames,
    fps,
  );
  const wav = path.join(publicDirectory, "mix.wav");
  trimAudio(project.audio, wav, clip);
  const mix = samples(wav, { ...clip, startSeconds: 0 });
  const scene = {
    fps,
    frames,
    duration: clip.durationSeconds,
    fadeOut: clip.fadeOutSeconds,
    visual: project.visual,
    audio: `${id}/mix.wav`,
    presentation: project.presentation,
    sections: project.sections,
    tracks,
    bands: bands(mix, frames, fps),
    energy: levels(mix, frames, fps),
  };
  writeFileSync(
    path.join(publicDirectory, "scene.json"),
    JSON.stringify(scene),
  );
  const propsDirectory = path.join(toolDirectory, "out", ".cache", id);
  mkdirSync(propsDirectory, { recursive: true });
  const propsPath = path.join(propsDirectory, "props.json");
  writeFileSync(
    propsPath,
    JSON.stringify({ scenePath: `${id}/scene.json` }, null, 2),
  );
  console.log(
    `Prepared ${clip.durationSeconds}s / ${fps}fps / ${tracks.length} tracks: ${id}`,
  );
  return propsPath;
}

export function renderInstrumentScore(project, command, time) {
  if (time !== undefined && command !== "still")
    throw new Error("time is only supported by still");
  const seconds =
    time === undefined ? project.clip.durationSeconds * 0.75 : Number(time);
  if (
    command === "still" &&
    (!Number.isFinite(seconds) ||
      seconds < 0 ||
      seconds >= project.clip.durationSeconds)
  )
    throw new Error("still time must be within the video");

  const props = prepareInstrumentScore(project);
  const entry = path.join(toolDirectory, "src/index.tsx");
  if (command === "prepare") return;
  if (command === "studio") {
    runRemotion(["studio", entry], props);
    return;
  }
  if (command === "still") {
    runRemotion(
      [
        "still",
        entry,
        "InstrumentScore",
        path.join(project.outputDirectory, project.outputs.poster),
        `--frame=${Math.floor(seconds * project.fps)}`,
      ],
      props,
    );
    return;
  }

  const master = path.join(project.outputDirectory, project.outputs.master);
  runRemotion(
    [
      "render",
      entry,
      "InstrumentScore",
      master,
      "--codec=h264",
      "--crf=20",
      "--pixel-format=yuv420p",
      "--audio-bitrate=256k",
      "--concurrency=4",
    ],
    props,
  );
  exportVideo(project, master);
}
