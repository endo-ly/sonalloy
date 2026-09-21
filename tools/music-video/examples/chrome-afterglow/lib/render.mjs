import path from "node:path";
import { loadProject } from "./project.mjs";
import { prepare } from "./prepare.mjs";
import { exportVideo } from "./export.mjs";
import { runRemotion, toolDirectory } from "../../../lib/remotion.mjs";

export function renderInstrumentScore(command, filename, time) {
  if (!["prepare", "studio", "still", "render"].includes(command) || !filename)
    throw new Error(
      "usage: node scripts/render.mjs <command> <config/project.json> [still-time-seconds]",
    );

  const project = loadProject(filename);
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

  const props = prepare(project);
  const entry = path.join(toolDirectory, "src/index.tsx");

  if (command === "studio") runRemotion(["studio", entry], props);
  if (command === "still")
    runRemotion([
      "still",
      entry,
      "InstrumentScore",
      path.join(project.outputDirectory, project.outputs.poster),
      `--frame=${Math.floor(seconds * project.fps)}`,
    ], props);
  if (command === "render") {
    const master = path.join(project.outputDirectory, project.outputs.master);
    runRemotion([
      "render",
      entry,
      "InstrumentScore",
      master,
      "--codec=h264",
      "--crf=20",
      "--pixel-format=yuv420p",
      "--audio-bitrate=256k",
      "--concurrency=4",
    ], props);
    exportVideo(project, master);
  }
}
