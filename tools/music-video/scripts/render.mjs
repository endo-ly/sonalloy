import { renderInstrumentScore } from "../lib/instrument-score.mjs";
import { loadProject, resolveRender } from "../lib/project.mjs";
import { renderVisualizer } from "../lib/visualizer.mjs";

const commands = new Set(["prepare", "studio", "still", "render"]);

function usage() {
  return "usage: node scripts/render.mjs <prepare|studio|still|render> <project.json> [render-id] [variant|still-seconds] [still-seconds]";
}

export function render(args = process.argv.slice(2)) {
  const [command, filename, renderId, option, stillTime] = args;
  if (!commands.has(command) || !filename) throw new Error(usage());

  const project = loadProject(filename);
  if (!renderId && ["studio", "still"].includes(command))
    throw new Error(`${command} requires a render-id`);
  const renderIds = renderId
    ? [renderId]
    : project.renders.map((render) => render.id);

  for (const id of renderIds) {
    const target = resolveRender(project, id);
    if (target.renderer === "instrument-score") {
      renderInstrumentScore(
        target,
        command,
        command === "still" ? option : undefined,
      );
      continue;
    }
    renderVisualizer(
      target,
      command,
      option ?? "all",
      command === "still" ? stillTime ?? "12" : "12",
    );
  }
}

try {
  render();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
