import { Sculpture } from "../visualizers/Sculpture";
import React from "react";
import {
  AbsoluteFill,
  Audio,
  Composition,
  staticFile,
  useCurrentFrame,
} from "remotion";

type Note = {
  start: number;
  duration: number;
  pitch: number;
  velocity: number;
};
type Track = {
  id: string;
  name: string;
  category: string;
  kind: string;
  color: string;
  energy: number[] | null;
  notes: Note[];
};
type ElementSpec = {
  id: string;
  type: string;
  x: number;
  y: number;
  width: number;
  height: number;
  visible: boolean;
  bind: string | null;
  text: string | null;
  style: Record<string, any>;
};
type Scene = {
  fps: number;
  frames: number;
  duration: number;
  fadeOut: number;
  audio: string;
  presentation: Record<string, string>;
  sections: { at: number; label: string; color: string; emphasis: boolean }[];
  tracks: Track[];
  bands: number[][];
  energy: number[];
  visual: {
    canvas: { width: number; height: number };
    delivery: { width: number; height: number };
    theme: {
      background: string;
      foreground: string;
      muted: string;
      line: string;
      accent: string;
      fontFamily: string;
      monoFamily: string;
      gridOpacity: number;
      cornerRadius: number;
    };
    elements: ElementSpec[];
  };
};
type Props = { scenePath: string; data: Scene | null };

const clamp = (x: number) => Math.max(0, Math.min(1, x));
const ease = (x: number) => {
  const t = clamp(x);
  return t * t * (3 - 2 * t);
};
const activity = (track: Track, frame: number, fps: number) =>
  track.energy?.[frame] ??
  track.notes.reduce(
    (peak, note) =>
      frame / fps >= note.start && frame / fps < note.start + note.duration
        ? Math.max(peak, note.velocity / 127)
        : peak,
    0,
  );
const binding = (data: Scene, element: ElementSpec) =>
  element.text ?? (element.bind ? (data.presentation[element.bind] ?? "") : "");
const boxStyle = (element: ElementSpec): React.CSSProperties => ({
  position: "absolute",
  left: element.x,
  top: element.y,
  width: element.width,
  height: element.height,
  overflow: "hidden",
});

function LevelTimeline({
  energy,
  frame,
  color,
  width,
  height,
  line,
}: {
  energy: number[];
  frame: number;
  color: string;
  width: number;
  height: number;
  line: string;
}) {
  const peak = Math.max(0.001, ...energy);
  return (
    <svg width={width} height={height}>
      {Array.from({ length: 160 }, (_, i) => {
        const index = Math.floor((i / 160) * energy.length);
        const level = (energy[index] ?? 0) / peak;
        return (
          <rect
            key={i}
            x={(i / 160) * width}
            y={(height * (1 - level)) / 2}
            width={Math.max(1, width / 160 - 2)}
            height={Math.max(1, level * height)}
            fill={color}
            opacity={index <= frame ? 0.8 : 0.2}
          />
        );
      })}
      <line
        x1={(frame / Math.max(1, energy.length)) * width}
        x2={(frame / Math.max(1, energy.length)) * width}
        y1={0}
        y2={height}
        stroke={line}
      />
    </svg>
  );
}

function Instrument({
  track,
  index,
  frame,
  fps,
  width,
  height,
  theme,
}: {
  track: Track;
  index: number;
  frame: number;
  fps: number;
  width: number;
  height: number;
  theme: Scene["visual"]["theme"];
}) {
  const seconds = frame / fps;
  const level = activity(track, frame, fps);
  const pitches = track.notes.map((note) => note.pitch);
  const min = pitches.length ? Math.min(...pitches) - 2 : 0;
  const max = pitches.length ? Math.max(...pitches) + 2 : 127;
  const chartWidth = Math.min(304, width - 32);
  const chartHeight = Math.max(30, height - 133);
  const playhead = 78;
  const speed = 81;
  const cardColor =
    track.color +
    Math.round(5 + level * 15)
      .toString(16)
      .padStart(2, "0");
  return (
    <div
      style={{
        width,
        height,
        borderTop: "2px solid " + track.color,
        paddingTop: 20,
        borderRadius: theme.cornerRadius,
        background: "linear-gradient(180deg, " + cardColor + ", transparent)",
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          padding: "0 16px",
        }}
      >
        <span
          style={{
            fontFamily: theme.monoFamily,
            fontSize: 13,
            color: track.color,
            letterSpacing: 2,
          }}
        >
          {String(index + 1).padStart(2, "0")}
        </span>
        <div style={{ height: 6, width: 54, background: "#252c35" }}>
          <div
            style={{
              height: "100%",
              width: String(level * 100) + "%",
              background: track.color,
            }}
          />
        </div>
      </div>
      <div
        style={{
          fontSize: Math.min(
            26,
            ((width - 32) / Math.max(1, track.name.length)) * 1.7,
          ),
          fontWeight: 600,
          letterSpacing: -0.7,
          color: level > 0.02 ? theme.foreground : "#afb3bd",
          margin: "13px 16px 5px",
        }}
      >
        {track.name}
      </div>
      <div
        style={{
          fontFamily: theme.monoFamily,
          fontSize: 11,
          color: theme.muted,
          letterSpacing: 2,
          marginLeft: 17,
        }}
      >
        {track.category}
      </div>
      <div style={{ margin: "19px 16px 0" }}>
        {!track.notes.length && track.energy ? (
          <LevelTimeline
            energy={track.energy}
            frame={frame}
            color={track.color}
            width={chartWidth}
            height={chartHeight}
            line={theme.foreground}
          />
        ) : (
          <svg
            width={chartWidth}
            height={chartHeight}
            style={{ overflow: "hidden" }}
          >
            {[0, 1, 2, 3].map((i) => (
              <line
                key={i}
                x1={0}
                x2={chartWidth}
                y1={i * 23 + 8}
                y2={i * 23 + 8}
                stroke="#363d48"
                strokeOpacity="0.45"
              />
            ))}
            {track.notes
              .filter(
                (note) =>
                  note.start + note.duration > seconds - 1 &&
                  note.start < seconds + 3,
              )
              .map((note, i) => (
                <rect
                  key={i}
                  x={playhead + (note.start - seconds) * speed}
                  y={
                    6 +
                    (1 - (note.pitch - min) / Math.max(1, max - min)) *
                      64
                  }
                  width={Math.max(5, note.duration * speed - 2)}
                  height={5}
                  rx={1}
                  fill={track.color}
                  opacity={
                    seconds >= note.start &&
                    seconds < note.start + note.duration
                      ? 1
                      : note.start < seconds
                        ? 0.22
                        : 0.46
                  }
                />
              ))}
            <line
              x1={playhead}
              x2={playhead}
              y1={0}
              y2={chartHeight}
              stroke={theme.foreground}
              strokeOpacity="0.55"
            />
            <circle cx={playhead} cy={2} r={2} fill={theme.foreground} />
          </svg>
        )}
      </div>
    </div>
  );
}

function TrackGrid({
  data,
  frame,
  element,
}: {
  data: Scene;
  frame: number;
  element: ElementSpec;
}) {
  const tracks = data.tracks.filter((track) => track.kind === "melody");
  const style = element.style;
  const label =
    style.label ??
    (tracks.length ? "INSTRUMENTS / NOTE TIMELINE" : "MIX / LEVEL TIMELINE");
  const gap = Number(style.gap ?? 10);
  if (!tracks.length)
    return (
      <div>
        <div
          style={{
            fontFamily: data.visual.theme.monoFamily,
            fontSize: 12,
            color: data.visual.theme.muted,
            letterSpacing: 2,
            marginBottom: Number(style.labelMarginBottom ?? 18),
          }}
        >
          {label}
        </div>
        <LevelTimeline
          energy={data.energy}
          frame={frame}
          color={data.visual.theme.accent}
          width={element.width}
          height={Math.max(40, element.height - 30)}
          line={data.visual.theme.foreground}
        />
      </div>
    );
  const columns = Math.max(
    1,
    Math.min(tracks.length, Math.floor(style.columns ?? tracks.length)),
  );
  const width = (element.width - gap * (columns - 1)) / columns;
  const rows = Math.ceil(tracks.length / columns);
  const height = Math.max(60, (element.height - 30 - gap * (rows - 1)) / rows);
  return (
    <div>
      <div
          style={{
            fontFamily: data.visual.theme.monoFamily,
            fontSize: 12,
            color: data.visual.theme.muted,
            letterSpacing: 2,
            marginBottom: Number(style.labelMarginBottom ?? 18),
          }}
        >
          <span>{label}</span>
          {data.presentation.credit ? (
            <span style={{ float: "right" }}>{data.presentation.credit}</span>
          ) : null}
      </div>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(" + columns + ", " + width + "px)",
          gap,
        }}
      >
        {tracks.map((track, index) => (
          <Instrument
            key={track.id}
            track={track}
            index={index}
            frame={frame}
            fps={data.fps}
            width={width}
            height={height}
            theme={data.visual.theme}
          />
        ))}
      </div>
    </div>
  );
}

function DrumStrip({
  data,
  frame,
  element,
}: {
  data: Scene;
  frame: number;
  element: ElementSpec;
}) {
  const tracks = data.tracks.filter((track) => track.kind === "percussion");
  if (!tracks.length) return null;
  const seconds = frame / data.fps;
  const gap = Number(element.style.gap ?? 20);
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "100px repeat(" + tracks.length + ", 1fr)",
        gap,
        alignItems: "center",
        height: element.height,
      }}
    >
      <span
        style={{
          fontFamily: data.visual.theme.monoFamily,
          fontSize: 12,
          color: data.visual.theme.muted,
          letterSpacing: 2,
        }}
      >
        {element.style.label ?? "DRUMS"}
      </span>
      {tracks.map((track) => {
        const pulse = track.notes.length
          ? track.notes.reduce(
              (peak, note) =>
                note.start <= seconds
                  ? Math.max(
                      peak,
                      (Math.exp(-(seconds - note.start) * 16) * note.velocity) /
                        127,
                    )
                  : peak,
              0,
            )
          : activity(track, frame, data.fps);
        return (
          <div
            key={track.id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              minWidth: 0,
            }}
          >
            <div
              style={{
                width: 7,
                height: 7,
                borderRadius: "50%",
                background: track.color,
                opacity: 0.18 + pulse * 0.82,
                boxShadow: "0 0 " + pulse * 12 + "px " + track.color,
                flexShrink: 0,
              }}
            />
            <span
              style={{
                fontSize: tracks.length > 4 ? 13 : 16,
                color:
                  pulse > 0.15
                    ? data.visual.theme.foreground
                    : "#929ba8",
                letterSpacing: 0.2,
                whiteSpace: "nowrap",
              }}
            >
              {track.name}
            </span>
            <div
              style={{
                width: 46,
                height: 3,
                background: data.visual.theme.line,
                flexShrink: 0,
                marginLeft: 5,
              }}
            >
              <div
                style={{
                  width:
                    String(
                      (track.energy?.[frame] ?? activity(track, frame, data.fps)) *
                        100,
                    ) + "%",
                  height: "100%",
                  background: track.color,
                }}
              />
            </div>
          </div>
        );
      })}
    </div>
  );
}

function TextElement({
  data,
  element,
  type,
  frame,
}: {
  data: Scene;
  element: ElementSpec;
  type: string;
  frame: number;
}) {
  const theme = data.visual.theme;
  const style = element.style;
  const intro =
    style.animate === false ? 1 : ease((frame / data.fps + 0.8) / 1.133);
  const text = binding(data, element);
  const fontSize = Number(
    style.fontSize ??
      (
        {
          title: Math.min(143, 1100 / Math.max(8, text.length)),
          subtitle: 50,
          description: 22,
          eyebrow: 16,
        } as Record<string, number>
      )[type] ??
      18,
  );
  const color =
    style.color ??
    (type === "eyebrow"
      ? theme.accent
      : type === "description"
        ? theme.muted
        : theme.foreground);
  return (
    <div
      style={{
        ...boxStyle(element),
        opacity: intro,
        transform:
          "translateY(" +
          (1 - intro) * Number(style.introDistance ?? 20) +
          "px)",
        fontFamily:
          style.fontFamily ??
          (type === "eyebrow" ? theme.monoFamily : theme.fontFamily),
        fontSize,
        fontWeight: style.fontWeight ?? (type === "title" ? 700 : 400),
        lineHeight: style.lineHeight ?? (type === "title" ? 1 : 1.15),
        letterSpacing:
          style.letterSpacing ??
          (type === "title" ? -4 : type === "eyebrow" ? 2 : 0),
        color,
        textAlign: style.align ?? "left",
        whiteSpace: style.wrap === false ? "nowrap" : "normal",
      }}
    >
      {text}
      {style.suffix ? (
        <span style={{ color: style.suffixColor ?? theme.accent }}>
          {style.suffix}
        </span>
      ) : null}
    </div>
  );
}

function Header({ data, element }: { data: Scene; element: ElementSpec }) {
  const theme = data.visual.theme;
  return (
    <div
      style={{
        ...boxStyle(element),
        display: "flex",
        justifyContent: "space-between",
        alignItems: "flex-start",
        borderBottom: "1px solid " + theme.line,
        color: theme.muted,
        fontFamily: theme.monoFamily,
        fontSize: Number(element.style.fontSize ?? 14),
        letterSpacing: Number(element.style.letterSpacing ?? 2),
      }}
    >
      <span
        style={{
          fontSize: Number(element.style.leftFontSize ?? 16),
          color: element.style.leftColor ?? "#b5b9c1",
        }}
      >
        {String(element.style.left ?? data.presentation.label).split(" / ")[0]}{" "}
        <span
          style={{
            color: element.style.separatorColor ?? "#4d596a",
            padding: "0 18px",
          }}
        >
          /
        </span>
        {" "}{String(element.style.left ?? data.presentation.label)
          .split(" / ")
          .slice(1)
          .join(" / ")}
      </span>
      <span
        style={{
          fontSize: Number(element.style.rightFontSize ?? 14),
          transform: "translateY(1px)",
        }}
      >
        {String(element.style.right ?? data.presentation.detail).split(" — ")[0]}{" "}
        <span
          style={{
            color: element.style.rightSeparatorColor ?? "#424c58",
            padding: "0 16px",
          }}
        >
          —
        </span>
        {" "}{String(element.style.right ?? data.presentation.detail)
          .split(" — ")
          .slice(1)
          .join(" — ")}
      </span>
    </div>
  );
}

function Sections({
  data,
  element,
  frame,
}: {
  data: Scene;
  element: ElementSpec;
  frame: number;
}) {
  if (!data.sections.length) return null;
  const seconds = frame / data.fps;
  const current =
    data.sections.filter((section) => section.at <= seconds).length - 1;
  const gap = Number(element.style.gap ?? 10);
  const width =
    (element.width - gap * (data.sections.length - 1)) / data.sections.length;
  return (
    <div style={{ ...boxStyle(element), display: "flex", gap }}>
      {data.sections.map((section, index) => (
        <div
          key={section.at}
          style={{
            width,
            color: index === current ? data.visual.theme.foreground : "#626c7b",
            fontFamily: data.visual.theme.monoFamily,
            fontSize: Number(element.style.fontSize ?? 11),
            letterSpacing: 1,
          }}
        >
          <div
            style={{
              height: Number(element.style.lineHeight ?? 2),
              background:
                index === current ? section.color : "#303946",
              marginBottom: 12,
            }}
          />
          {section.label}
        </div>
      ))}
    </div>
  );
}

function Spectrum({
  data,
  element,
  frame,
}: {
  data: Scene;
  element: ElementSpec;
  frame: number;
}) {
  const spectrum = data.bands[frame] ?? [];
  const theme = data.visual.theme;
  const melodyTracks = data.tracks.filter((track) => track.kind === "melody");
  const baseline = Number(element.style.baseline ?? element.height - 24);
  const barHeight = Number(element.style.barHeight ?? element.height - 34);
  return (
    <div
      style={{
        ...boxStyle(element),
        fontFamily: theme.monoFamily,
        fontSize: Number(element.style.fontSize ?? 13),
        color: theme.muted,
      }}
    >
      <svg width={element.width} height={element.height}>
        <line
          x1={0}
          x2={element.width}
          y1={baseline}
          y2={baseline}
          stroke="#687181"
          strokeOpacity="0.26"
        />
        {spectrum.map((level, index) => (
          <rect
            key={index}
            x={index * 11.7}
            y={baseline - level * barHeight}
            width={3}
            height={Math.max(1, level * barHeight)}
            fill={
              melodyTracks[Math.floor(index / 13) % Math.max(1, melodyTracks.length)]?.color ??
              theme.accent
            }
            opacity={0.3 + level * 0.55}
          />
        ))}
          <text
            x={0}
            y={57}
            fill={theme.muted}
            fontSize={13}
            fontFamily={theme.monoFamily}
            letterSpacing={2}
          >
            {element.style.leftLabel ?? "LIVE SPECTRUM"}
          </text>
          <text
            x={element.width - 2}
            y={57}
            textAnchor="end"
            fill={theme.muted}
            fontSize={13}
            fontFamily={theme.monoFamily}
            letterSpacing={2}
          >
            {element.style.rightLabel ?? "45 Hz — 9.5 kHz"}
          </text>
        </svg>
    </div>
  );
}

function Footer({
  data,
  element,
  frame,
}: {
  data: Scene;
  element: ElementSpec;
  frame: number;
}) {
  const seconds = frame / data.fps;
  return (
    <div
      style={{
        ...boxStyle(element),
        display: "flex",
        justifyContent: "space-between",
        alignItems: "flex-start",
        color: data.visual.theme.muted,
        fontFamily: data.visual.theme.monoFamily,
        fontSize: Number(element.style.fontSize ?? 12),
        letterSpacing: 2,
      }}
    >
      <span>{data.presentation.footer}</span>
      <span>{seconds.toFixed(0).padStart(2, "0")} / {data.duration} SEC</span>
    </div>
  );
}

function ElementView({
  data,
  element,
  frame,
}: {
  data: Scene;
  element: ElementSpec;
  frame: number;
}) {
  if (!element.visible) return null;
  switch (element.type) {
    case "header":
      return <Header data={data} element={element} />;
    case "eyebrow":
    case "title":
    case "subtitle":
    case "description":
    case "text":
      return (
        <TextElement
          data={data}
          element={element}
          type={element.type}
          frame={frame}
        />
      );
    case "sections":
      return <Sections data={data} element={element} frame={frame} />;
    case "sculpture":
      return (
        <div style={boxStyle(element)}>
          <Sculpture
            data={data}
            frame={frame}
            width={element.width}
            height={element.height}
            style={element.style}
          />
        </div>
      );
    case "spectrum":
      return <Spectrum data={data} element={element} frame={frame} />;
    case "tracks":
    case "waveform":
      return (
        <div style={boxStyle(element)}>
          <TrackGrid data={data} frame={frame} element={element} />
        </div>
      );
    case "drums":
      return (
        <div style={boxStyle(element)}>
          <DrumStrip data={data} frame={frame} element={element} />
        </div>
      );
    case "progress":
      return (
        <div
          style={{ ...boxStyle(element), background: data.visual.theme.line }}
        >
          <div
            style={{
              height: element.height,
              width: String((frame / Math.max(1, data.frames - 1)) * 100) + "%",
              background: element.style.color ?? data.visual.theme.accent,
            }}
          />
        </div>
      );
    case "footer":
      return <Footer data={data} element={element} frame={frame} />;
    default:
      return null;
  }
}

function Film({ data }: Props) {
  const frame = useCurrentFrame();
  if (!data) return null;
  const theme = data.visual.theme;
  const finish = data.fadeOut
    ? 1 -
      ease((frame / data.fps - data.duration + data.fadeOut) / data.fadeOut) *
        0.75
    : 1;
  return (
    <AbsoluteFill
      style={{
        background: theme.background,
        color: theme.foreground,
        fontFamily: theme.fontFamily,
        overflow: "hidden",
      }}
    >
      <Audio src={staticFile(data.audio)} />
      <div style={{ position: "absolute", inset: 0, opacity: finish }}>
        <div
          style={{
            position: "absolute",
            inset: 0,
            backgroundImage:
              "radial-gradient(#ffffff12 0.7px, transparent 0.7px)",
            backgroundSize: "6px 6px",
            opacity: theme.gridOpacity,
          }}
        />
        {data.visual.elements.map((element) => (
          <ElementView
            key={element.id}
            data={data}
            element={element}
            frame={frame}
          />
        ))}
      </div>
    </AbsoluteFill>
  );
}

export function InstrumentScoreComposition() {
  return (
    <Composition
      id="InstrumentScore"
      component={Film}
      defaultProps={{ scenePath: "", data: null } as Props}
      width={1920}
      height={1080}
      fps={30}
      durationInFrames={1}
      calculateMetadata={async ({ props, abortSignal }) => {
        if (!props.scenePath)
          throw new Error("Provide a prepared project using scripts/render.mjs");
        const response = await fetch(staticFile(props.scenePath), {
          signal: abortSignal,
        });
        if (!response.ok) throw new Error("Cannot load " + props.scenePath);
        const data: Scene = await response.json();
        return {
          fps: data.fps,
          durationInFrames: data.frames,
          width: data.visual.canvas.width,
          height: data.visual.canvas.height,
          props: { ...props, data },
        };
      }}
    />
  );
}
