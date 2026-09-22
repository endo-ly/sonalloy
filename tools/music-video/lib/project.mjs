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
function optionalText(value, field) {
  if (value === undefined || value === null) return undefined;
  return text(value, field);
}
function stringArray(value, field) {
  if (value === undefined || value === null) return undefined;
  if (
    !Array.isArray(value) ||
    value.some((item) => typeof item !== "string" || !item.trim())
  )
    fail(field, "expected an array of non-empty text");
  return value;
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


function sectionsConfig(input, duration, fps, accent, field) {
  if (input !== undefined && !Array.isArray(input))
    fail(field, "expected array");
  return (input ?? []).map((section, index, all) => {
    const sectionField = `${field}[${index}]`;
    const at = number(
      section.at,
      `${sectionField}.at`,
      0,
      duration - 1 / fps,
    );
    if (
      (index === 0 && at !== 0) ||
      (index > 0 && at <= all[index - 1].at)
    )
      fail(field, "start at 0 and use increasing times");
    if (section.emphasis !== undefined && typeof section.emphasis !== "boolean")
      fail(`${sectionField}.emphasis`, "expected boolean");
    const label = section.label ?? "";
    if (typeof label !== "string")
      fail(`${sectionField}.label`, "expected text");
    return {
      at,
      label,
      color: color(
        section.color ?? accent,
        `${sectionField}.color`,
      ),
      emphasis: section.emphasis ?? false,
    };
  });
}

function presentationConfig(input, accent, field, defaultTitle = "Music Video") {
  const value = input ?? {};
  const result = {
    title: text(value.title, `${field}.title`, defaultTitle),
    label: text(value.label, `${field}.label`, "MUSIC / VISUAL STUDY"),
    subtitle: text(
      value.subtitle,
      `${field}.subtitle`,
      "Sound in motion.",
    ),
    description: value.description ?? "",
    eyebrow: value.eyebrow ?? "",
    detail: value.detail ?? "",
    credit: value.credit ?? "",
    footer: value.footer ?? "",
    accent: color(value.accent ?? accent, `${field}.accent`),
  };
  for (const [key, item] of Object.entries(result))
    if (typeof item !== "string") fail(`${field}.${key}`, "expected text");
  return result;
}

export function loadProject(filename) {
  const configPath = path.resolve(filename);
  const directory = path.dirname(configPath);
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
      "must contain a whole number of frames",
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

  const audio = file(directory, config.audio, "audio");
  const title = text(config.title, "title", id);
  if (!Array.isArray(config.tracks) || config.tracks.length === 0)
    fail("tracks", "expected a non-empty array");

  const trackIds = new Set();
  const tracks = config.tracks.map((track, index) => {
    const field = `tracks[${index}]`;
    const trackId = text(track.id, `${field}.id`);
    if (trackIds.has(trackId)) fail(`${field}.id`, "duplicate identifier");
    trackIds.add(trackId);
    if (!track.audio) fail(`${field}.audio`, "is required");
    const notes = track.notes ?? track.pattern;
    const kind = optionalText(track.kind, `${field}.kind`);
    const category = optionalText(track.category, `${field}.category`);
    const tags = stringArray(track.tags, `${field}.tags`);
    return {
      id: trackId,
      name: text(track.name, `${field}.name`, trackId),
      kind,
      category,
      tags,
      color: color(track.color ?? "#88ded3", `${field}.color`),
      audio: file(directory, track.audio, `${field}.audio`),
      notes:
        notes === undefined
          ? undefined
          : file(directory, notes, `${field}.notes`),
    };
  });

  const accent = "#88ded3";
  const sections = sectionsConfig(
    config.sections,
    duration,
    fps,
    accent,
    "sections",
  );

  if (!Array.isArray(config.renders) || config.renders.length === 0)
    fail("renders", "expected a non-empty array");
  const renderIds = new Set();
  const renders = config.renders.map((render, index) => {
    const field = `renders[${index}]`;
    const renderId = text(render.id, `${field}.id`);
    if (renderIds.has(renderId))
      fail(`${field}.id`, "duplicate identifier");
    renderIds.add(renderId);
    if (!["instrument-score", "visualizer"].includes(render.renderer))
      fail(
        `${field}.renderer`,
        "expected instrument-score or visualizer",
      );
    if (
      render.trackIds !== undefined &&
      render.trackIds !== "all" &&
      (!Array.isArray(render.trackIds) ||
        render.trackIds.some((trackId) => !trackIds.has(trackId)))
    )
      fail(`${field}.trackIds`, "contains an unknown track");
    if (
      render.variants !== undefined &&
      (!Array.isArray(render.variants) ||
        render.variants.some((variant) => typeof variant !== "string"))
    )
      fail(`${field}.variants`, "expected an array of names");
    return render;
  });

  return {
    id,
    title,
    configPath,
    directory,
    fps,
    frames,
    clip,
    audio,
    tracks,
    sections,
    renders,
  };
}

export function resolveRender(project, renderId) {
  const render = project.renders.find((item) => item.id === renderId);
  if (!render) throw new Error(`unknown render: ${renderId}`);
  const selectedIds = render.trackIds ?? "all";
  const tracks =
    selectedIds === "all"
      ? project.tracks
      : project.tracks.filter((track) => selectedIds.includes(track.id));
  const accent = render.presentation?.accent ?? "#88ded3";
  const presentation = presentationConfig(
    render.presentation,
    accent,
    `renders.${render.id}.presentation`,
    project.title,
  );
  const sections = sectionsConfig(
    render.sections ?? project.sections,
    project.clip.durationSeconds,
    project.fps,
    accent,
    `renders.${render.id}.sections`,
  );
  const visual =
    render.renderer === "instrument-score"
      ? visualConfig(render.visual, presentation.accent)
      : null;
  const width = number(
    render.width ?? 1080,
    `renders.${render.id}.width`,
    320,
    7680,
  );
  const height = number(
    render.height ?? 1920,
    `renders.${render.id}.height`,
    180,
    7680,
  );
  if (!Number.isInteger(width) || !Number.isInteger(height))
    fail(`renders.${render.id}`, "width and height must be integers");
  if (render.renderer === "visualizer" && width * 16 !== height * 9)
    fail(`renders.${render.id}`, "visualizer output must use a 9:16 aspect ratio");
  return {
    ...project,
    id: render.cacheId ?? `${project.id}-${render.id}`,
    renderId: render.id,
    renderer: render.renderer,
    tracks,
    sections,
    presentation,
    visual,
    title: render.title ?? project.title,
    outputDirectory: path.resolve(
      project.directory,
      render.outputDirectory ?? `../../out/${project.id}/${render.id}`,
    ),
    outputs: {
      master: text(
        render.outputs?.master,
        `renders.${render.id}.outputs.master`,
        "master.mp4",
      ),
      video: text(
        render.outputs?.video,
        `renders.${render.id}.outputs.video`,
        "video.mp4",
      ),
      poster: text(
        render.outputs?.poster,
        `renders.${render.id}.outputs.poster`,
        "poster.png",
      ),
    },
    width,
    height,
    variants: render.variants ?? [],
  };
}
