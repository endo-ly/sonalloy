import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { bands, levels, samples, trimAudio } from "../../../lib/audio.mjs";
import { toolDirectory } from "../../../lib/remotion.mjs";
import { prepareTracks } from "../../../lib/tracks.mjs";

export function prepare(project) {
  const { id, clip, frames, fps } = project;
  const publicDirectory = path.join(toolDirectory, "public", id);
  mkdirSync(publicDirectory, { recursive: true });
  mkdirSync(project.outputDirectory, { recursive: true });
  const tracks = prepareTracks(
    project.tracks,
    path.dirname(project.configPath),
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
