import { readFileSync, statSync } from "node:fs";
import path from "node:path";

const fail = (field, message) => {
  throw new Error(`${field}: ${message}`);
};
function text(value, field, fallback) {
  const result = value ?? fallback;
  if (typeof result !== "string" || !result.trim())
    fail(field, "expected non-empty text");
  return result;
}
function number(value, field, min, max) {
  if (!Number.isFinite(value) || value < min || value > max)
    fail(field, `expected ${min}..${max}`);
  return value;
}
function color(value, field) {
  if (typeof value !== "string" || !/^#[0-9a-f]{6}$/i.test(value))
    fail(field, "expected #RRGGBB");
  return value;
}
function file(base, value, field) {
  const resolved = path.resolve(base, text(value, field));
  if (!statSync(resolved, { throwIfNoEntry: false })?.isFile())
    fail(field, `file not found: ${resolved}`);
  return resolved;
}

const elementTypes = new Set([
  "header",
  "eyebrow",
  "title",
  "subtitle",
  "description",
  "sections",
  "sculpture",
  "spectrum",
  "tracks",
  "drums",
  "progress",
  "footer",
  "text",
  "waveform",
]);

const defaultElements = [
  {
    id: "sculpture",
    type: "sculpture",
    x: 0,
    y: 0,
    width: 1920,
    height: 720,
    style: {
      centerX: 0.7161458333,
      centerY: 0.4305555556,
      ellipseRx: 465,
      ellipseRy: 270,
      circleRadii: [200, 245],
    },
  },
  {
    id: "eyebrow",
    type: "eyebrow",
    bind: "eyebrow",
    x: 80,
    y: 114,
    width: 780,
    height: 28,
    style: { letterSpacing: 2 },
  },
  {
    id: "title",
    type: "title",
    bind: "title",
    x: 80,
    y: 157,
    width: 780,
    height: 150,
    style: { suffix: ".", fontSize: 143, letterSpacing: -10, lineHeight: 1 },
  },
  {
    id: "subtitle",
    type: "subtitle",
    bind: "subtitle",
    x: 80,
    y: 334,
    width: 780,
    height: 64,
    style: { letterSpacing: -1.6 },
  },
  {
    id: "description",
    type: "description",
    bind: "description",
    x: 80,
    y: 415,
    width: 780,
    height: 35,
    style: { letterSpacing: 0.1, color: "#939aa6" },
  },
  { id: "sections", type: "sections", x: 80, y: 522, width: 662, height: 35 },
  {
    id: "spectrum",
    type: "spectrum",
    x: 1000,
    y: 548,
    width: 750,
    height: 70,
    style: { baseline: 28, barHeight: 39 },
  },
  {
    id: "tracks",
    type: "tracks",
    x: 80,
    y: 649,
    width: 1760,
    height: 250,
    style: { label: "INSTRUMENTS / NOTE TIMELINE", labelMarginBottom: 25 },
  },
  { id: "drums", type: "drums", x: 80, y: 942, width: 1760, height: 28 },
];

function visualConfig(input, accent) {
  const visual = input ?? {};
  const canvas = visual.canvas ?? {};
  const width = number(canvas.width ?? 1920, "visual.canvas.width", 320, 7680);
  const height = number(
    canvas.height ?? 1080,
    "visual.canvas.height",
    180,
    7680,
  );
  if (!Number.isInteger(width) || !Number.isInteger(height))
    fail("visual.canvas", "width and height must be integers");
  const t = visual.theme ?? {};
  const theme = {
    background: color(t.background ?? "#0c1015", "visual.theme.background"),
    foreground: color(t.foreground ?? "#f1eee8", "visual.theme.foreground"),
    muted: color(t.muted ?? "#898e98", "visual.theme.muted"),
    line: color(t.line ?? "#29313d", "visual.theme.line"),
    accent: color(t.accent ?? accent, "visual.theme.accent"),
    fontFamily: text(
      t.fontFamily,
      "visual.theme.fontFamily",
      "Arial, sans-serif",
    ),
    monoFamily: text(
      t.monoFamily,
      "visual.theme.monoFamily",
      "Consolas, monospace",
    ),
    gridOpacity: number(
      t.gridOpacity ?? 0.24,
      "visual.theme.gridOpacity",
      0,
      1,
    ),
    cornerRadius: number(
      t.cornerRadius ?? 0,
      "visual.theme.cornerRadius",
      0,
      80,
    ),
  };
  const elementsInput = visual.elements ?? defaultElements;
  if (!Array.isArray(elementsInput) || !elementsInput.length)
    fail("visual.elements", "expected a non-empty array");
  const ids = new Set();
  const elements = elementsInput.map((element, i) => {
    const field = `visual.elements[${i}]`;
    const id = text(element.id, `${field}.id`);
    if (ids.has(id)) fail(`${field}.id`, "duplicate identifier");
    ids.add(id);
    if (!elementTypes.has(element.type))
      fail(`${field}.type`, `expected one of ${[...elementTypes].join(", ")}`);
    if (
      element.style !== undefined &&
      (typeof element.style !== "object" ||
        element.style === null ||
        Array.isArray(element.style))
    )
      fail(`${field}.style`, "expected object");
    return {
      id,
      type: element.type,
      x: number(element.x ?? 0, `${field}.x`, 0, width * 2),
      y: number(element.y ?? 0, `${field}.y`, 0, height * 2),
      width: number(element.width ?? width, `${field}.width`, 1, width * 2),
      height: number(
        element.height ?? height,
        `${field}.height`,
        1,
        height * 2,
      ),
      visible: element.visible ?? true,
      bind: element.bind ?? null,
      text: element.text ?? null,
      style: element.style ?? {},
    };
  });
  for (const element of elements)
    if (typeof element.visible !== "boolean")
      fail(`visual.elements.${element.id}.visible`, "expected boolean");
  const delivery = visual.delivery ?? {};
  const deliveryWidth = number(
    delivery.width ?? 1280,
    "visual.delivery.width",
    320,
    7680,
  );
  const deliveryHeight = number(
    delivery.height ?? Math.round((deliveryWidth * height) / width),
    "visual.delivery.height",
    180,
    7680,
  );
  return {
    canvas: { width, height },
    delivery: { width: deliveryWidth, height: deliveryHeight },
    theme,
    elements,
  };
}

export function loadProject(filename) {
  const configPath = path.resolve(filename);
  const base = path.dirname(configPath);
  const config = JSON.parse(readFileSync(configPath, "utf8"));
  const id = text(config.id, "id");
  if (
    !/^[a-z][a-z0-9_-]{0,63}$/.test(id) ||
    /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(id)
  )
    fail("id", "use a portable lowercase identifier");
  const fps = number(config.fps ?? 30, "fps", 1, 60);
  if (!Number.isInteger(fps)) fail("fps", "expected integer");
  const duration = number(
    config.clip?.durationSeconds,
    "clip.durationSeconds",
    1 / fps,
    600,
  );
  const frames = Math.round(duration * fps);
  if (Math.abs(frames - duration * fps) > 1e-6)
    fail(
      "clip.durationSeconds",
      "must contain a whole number of frames at the selected fps",
    );
  const clip = {
    startSeconds: number(
      config.clip?.startSeconds ?? 0,
      "clip.startSeconds",
      0,
      Number.MAX_SAFE_INTEGER,
    ),
    durationSeconds: duration,
    fadeOutSeconds: number(
      config.clip?.fadeOutSeconds ?? Math.min(0.5, duration),
      "clip.fadeOutSeconds",
      0,
      duration,
    ),
  };
  const accent = color(
    config.presentation?.accent ?? "#88ded3",
    "presentation.accent",
  );
  const p = config.presentation ?? {};
  const presentation = {
    title: text(p.title, "presentation.title"),
    label: text(p.label, "presentation.label", "MUSIC / VISUAL STUDY"),
    subtitle: text(p.subtitle, "presentation.subtitle", "Sound in motion."),
    description: p.description ?? "",
    eyebrow: p.eyebrow ?? "",
    detail: p.detail ?? "",
    credit: p.credit ?? "",
    footer: p.footer ?? "",
    accent,
  };
  for (const [key, value] of Object.entries(presentation))
    if (typeof value !== "string") fail(`presentation.${key}`, "expected text");
  const visual = visualConfig(config.visual, accent);
  const ids = new Set();
  if (config.tracks !== undefined && !Array.isArray(config.tracks))
    fail("tracks", "expected array");
  const tracks = (config.tracks ?? []).map((t, i) => {
    const field = `tracks[${i}]`;
    const trackId = text(t.id, `${field}.id`);
    if (ids.has(trackId)) fail(`${field}.id`, "duplicate identifier");
    ids.add(trackId);
    const kind = t.kind ?? "melody";
    if (!["melody", "percussion"].includes(kind))
      fail(`${field}.kind`, "expected melody or percussion");
    if (t.pattern && t.notes) fail(field, "choose pattern or notes");
    if (!t.audio && !t.pattern && !t.notes)
      fail(field, "provide audio, pattern or notes");
    return {
      id: trackId,
      name: text(t.name, `${field}.name`),
      kind,
      category: text(
        t.category,
        `${field}.category`,
        kind === "percussion" ? "PERCUSSION" : "INSTRUMENT",
      ),
      color: color(t.color ?? accent, `${field}.color`),
      audio: t.audio ? file(base, t.audio, `${field}.audio`) : null,
      pattern: t.pattern ? file(base, t.pattern, `${field}.pattern`) : null,
      notes: t.notes ? file(base, t.notes, `${field}.notes`) : null,
    };
  });
  for (const kind of ["melody", "percussion"])
    if (tracks.filter((t) => t.kind === kind).length > 6)
      fail("tracks", `at most 6 ${kind} tracks fit this template`);
  if (config.sections !== undefined && !Array.isArray(config.sections))
    fail("sections", "expected array");
  const sections = (config.sections ?? []).map((s, i, all) => {
    const at = number(s.at, `sections[${i}].at`, 0, duration - 1 / fps);
    if ((i === 0 && at !== 0) || (i > 0 && at <= all[i - 1].at))
      fail("sections", "start at 0 and use increasing times");
    if (s.emphasis !== undefined && typeof s.emphasis !== "boolean")
      fail(`sections[${i}].emphasis`, "expected boolean");
    return {
      at,
      label: text(s.label, `sections[${i}].label`),
      color: color(s.color ?? accent, `sections[${i}].color`),
      emphasis: s.emphasis ?? false,
    };
  });
  if (sections.length > 6)
    fail("sections", "at most 6 sections fit this template");
  const output = config.outputs ?? {};
  const outputs = {
    master: text(output.master, "outputs.master", "master.mp4"),
    video: text(output.video, "outputs.video", "video.mp4"),
    poster: text(output.poster, "outputs.poster", "poster.png"),
  };
  return {
    id,
    configPath,
    fps,
    frames,
    clip,
    presentation,
    visual,
    tracks,
    sections,
    audio: file(base, config.audio, "audio"),
    outputDirectory: path.resolve(
      base,
      text(config.outputDirectory, "outputDirectory", "out"),
    ),
    outputs,
  };
}
