import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const toolDirectory = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

const executable = path.join(
  toolDirectory,
  "node_modules/@remotion/cli/remotion-cli.js",
);

export function runRemotion(args, propsPath) {
  execFileSync(
    process.execPath,
    [executable, ...args, `--props=${propsPath}`],
    { cwd: toolDirectory, stdio: "inherit" },
  );
}
