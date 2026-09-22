import { execFileSync } from "node:child_process";

const rate = 22050;
export function scopes(pcm, frames, fps) {
  return Array.from({ length: frames }, (_, frame) => {
    const center = Math.floor(frame / fps * rate);
    let start = center;
    for (let i = center; i < center + 220 && i + 1 < pcm.length; i++) {
      if (pcm[i] <= 0 && pcm[i + 1] > 0) { start = i; break; }
    }
    return Array.from({ length: 160 }, (_, j) => Number((pcm[start + j * 3] ?? 0).toFixed(4)));
  });
}
export function audioDuration(file) {
  const probe = JSON.parse(
    execFileSync(
      "ffprobe",
      [
        "-v",
        "error",
        "-select_streams",
        "a:0",
        "-show_entries",
        "stream=codec_type:format=duration",
        "-of",
        "json",
        file,
      ],
      { encoding: "utf8" },
    ),
  );
  const duration = Number(probe.format?.duration);
  if (!probe.streams?.length || !Number.isFinite(duration) || duration <= 0)
    throw new Error(`cannot read audio duration: ${file}`);
  return duration;
}

export function trimAudio(input, output, clip) {
  if (audioDuration(input) + 0.001 < clip.startSeconds + clip.durationSeconds)
    throw new Error("clip exceeds the mix audio duration");
  const filters = [
    `atrim=start=${clip.startSeconds}:duration=${clip.durationSeconds}`,
    "asetpts=PTS-STARTPTS",
  ];
  if (clip.fadeOutSeconds > 0)
    filters.push(
      `afade=t=out:st=${clip.durationSeconds - clip.fadeOutSeconds}:d=${clip.fadeOutSeconds}`,
    );
  execFileSync("ffmpeg", [
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-i",
    input,
    "-af",
    filters.join(","),
    "-c:a",
    "pcm_s24le",
    output,
  ]);
}

export function samples(file, clip) {
  const bytes = execFileSync(
    "ffmpeg",
    [
      "-hide_banner",
      "-loglevel",
      "error",
      "-i",
      file,
      "-ss",
      String(clip.startSeconds),
      "-t",
      String(clip.durationSeconds),
      "-f",
      "f32le",
      "-ac",
      "1",
      "-ar",
      String(rate),
      "pipe:1",
    ],
    { maxBuffer: Math.ceil((clip.durationSeconds + 1) * rate * 4) },
  );
  return new Float32Array(
    bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
  );
}

export function levels(pcm, frames, fps) {
  return Array.from({ length: frames }, (_, frame) => {
    const center = Math.floor((frame / fps) * rate);
    const start = Math.max(0, center - 1024),
      end = Math.min(pcm.length, center + 256);
    let energy = 0;
    for (let i = start; i < end; i++) energy += pcm[i] * pcm[i];
    return Math.sqrt(energy / Math.max(1, end - start));
  });
}

function spectrum(pcm, frame, fps) {
  const size = 2048;
  const re = new Float64Array(size),
    im = new Float64Array(size);
  const center = Math.floor((frame / fps) * rate);
  for (let i = 0; i < size; i++)
    re[i] =
      (pcm[center + i - size / 2] ?? 0) *
      (0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (size - 1)));
  for (let i = 1, j = 0; i < size; i++) {
    let bit = size >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) [re[i], re[j]] = [re[j], re[i]];
  }
  for (let length = 2; length <= size; length *= 2) {
    const angle = (-2 * Math.PI) / length;
    for (let start = 0; start < size; start += length) {
      for (let j = 0; j < length / 2; j++) {
        const a = start + j,
          b = a + length / 2;
        const cos = Math.cos(angle * j),
          sin = Math.sin(angle * j);
        const tr = re[b] * cos - im[b] * sin,
          ti = re[b] * sin + im[b] * cos;
        re[b] = re[a] - tr;
        im[b] = im[a] - ti;
        re[a] += tr;
        im[a] += ti;
      }
    }
  }
  return Array.from({ length: 64 }, (_, band) => {
    const low = Math.max(
      1,
      Math.floor((45 * (9500 / 45) ** (band / 64) * size) / rate),
    );
    const high = Math.max(
      low + 1,
      Math.ceil((45 * (9500 / 45) ** ((band + 1) / 64) * size) / rate),
    );
    let magnitude = 0;
    for (let i = low; i < Math.min(high, size / 2); i++)
      magnitude = Math.max(magnitude, Math.hypot(re[i], im[i]) / size);
    return Math.max(
      0,
      Math.min(1, (20 * Math.log10(magnitude + 1e-8) + 65) / 55),
    );
  });
}

export function bands(pcm, frames, fps) {
  let previous = new Array(64).fill(0);
  const decay = 0.8 ** (30 / fps);
  return Array.from({ length: frames }, (_, frame) => {
    previous = spectrum(pcm, frame, fps).map((value, i) =>
      Math.max(value, previous[i] * decay),
    );
    return previous.map((v) => Number(v.toFixed(3)));
  });
}
