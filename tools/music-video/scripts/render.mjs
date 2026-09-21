import { readFileSync } from "node:fs";
import path from "node:path";
import { renderInstrumentScore } from "../examples/chrome-afterglow/lib/render.mjs";
import { renderVisualizer } from "../lib/visualizer.mjs";

const commands = new Set(["prepare", "studio", "still", "render"]);

function rendererFor(filename) {
  const config = JSON.parse(readFileSync(path.resolve(filename), "utf8"));
  if (config.renderer === "visualizer")
    return "visualizer";
  if (config.renderer === "instrument-score")
    return "instrument-score";
  throw new Error(
    "config.renderer must be visualizer or instrument-score",
  );
}

export function render(args = process.argv.slice(2)) {
  const [command, filename, option, stillTime] = args;
  if (!commands.has(command) || !filename)
    throw new Error(
      "usage: node scripts/render.mjs <prepare|studio|still|render> <config.json> [variant|all] [still-seconds]",
    );

  if (rendererFor(filename) === "visualizer")
    return renderVisualizer(command, filename, option ?? "all", stillTime ?? "12");
  return renderInstrumentScore(command, filename, option);
}

try {
  render();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
