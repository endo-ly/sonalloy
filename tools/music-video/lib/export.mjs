import { execFileSync } from "node:child_process";
import path from "node:path";

export function exportVideo(project, master) {
  const output = path.join(project.outputDirectory, project.outputs.video);
  const { width, height } = project.visual.delivery;
  execFileSync(
    "ffmpeg",
    [
      "-hide_banner",
      "-loglevel",
      "error",
      "-y",
      "-i",
      master,
      "-vf",
      `scale=${width}:${height}:flags=lanczos`,
      "-c:v",
      "libx264",
      "-preset",
      "slow",
      "-crf",
      "23",
      "-maxrate",
      "1800k",
      "-bufsize",
      "3600k",
      "-pix_fmt",
      "yuv420p",
      "-c:a",
      "aac",
      "-b:a",
      "192k",
      "-t",
      String(project.clip.durationSeconds),
      "-movflags",
      "+faststart",
      output,
    ],
    { stdio: "inherit" },
  );
  console.log(output);
}
