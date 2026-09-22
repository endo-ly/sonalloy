import type { Scene, Track, VisualTheme } from "./types";
const clamp = (x: number) => Math.max(0, Math.min(1, x));
const ease = (x: number) => {
  const t = clamp(x);
  return t * t * (3 - 2 * t);
};
const valueAt = (data: Scene, frame: number) => data.energy[frame] ?? 0;
const activity = (track: Track, frame: number, fps: number) =>
  track.activity?.[frame] ??
  track.notes.reduce(
    (peak, n) =>
      frame / fps >= n.start && frame / fps < n.start + n.duration
        ? Math.max(peak, n.velocity / 127)
        : peak,
    0,
  );
export function Sculpture({
  data,
  frame,
  width,
  height,
  style,
  theme,
}: {
  data: Scene;
  frame: number;
  width: number;
  height: number;
  style: Record<string, any>;
  theme: Pick<VisualTheme, "accent" | "background">;
}) {
  const seconds = frame / data.fps;
  const spectrum = data.bands[frame] ?? [];
  const current = data.sections
    .filter((section) => section.at <= seconds)
    .at(-1);
  const emphasis = current?.emphasis
    ? ease((seconds - (current?.at ?? 0)) / 0.4)
    : 0;
  const melodyTracks = data.tracks.filter((track) => track.kind === "melody");
  const palette = melodyTracks.length
    ? melodyTracks.map((track) => ({
        color: track.color,
        level: activity(track, frame, data.fps),
      }))
    : [{ color: theme.accent, level: valueAt(data, frame) * 5 }];
  const centerX = width * Number(style.centerX ?? 0.56);
  const centerY = height * Number(style.centerY ?? 0.46);
  const scale = Math.min(width, height) / 720;
  const ringCount = Math.max(12, Math.floor(style.ringCount ?? 70));
  const pointCount = Math.max(24, Math.floor(style.pointCount ?? 81));
  const project = (u: number, v: number) => {
    const level = spectrum[Math.floor((u / (2 * Math.PI)) * 63) % 64] ?? 0;
    const radius =
      (76 +
        level * (35 + emphasis * 20) +
        Math.min(1, valueAt(data, frame) * 5) * 8) *
      scale;
    const ring = (184 + emphasis * 14) * scale;
    const spin = seconds * Number(style.rotationSpeed ?? 0.14);
    let x = (ring + radius * Math.cos(v)) * Math.cos(u);
    let z = (ring + radius * Math.cos(v)) * Math.sin(u);
    let y = radius * Math.sin(v);
    [x, z] = [
      x * Math.cos(spin) - z * Math.sin(spin),
      x * Math.sin(spin) + z * Math.cos(spin),
    ];
    const tilt = 0.85 + 0.12 * Math.sin(seconds * 0.19);
    [y, z] = [
      y * Math.cos(tilt) - z * Math.sin(tilt),
      y * Math.sin(tilt) + z * Math.cos(tilt),
    ];
    [x, y] = [
      x * Math.cos(-0.35) - y * Math.sin(-0.35),
      x * Math.sin(-0.35) + y * Math.cos(-0.35),
    ];
    const perspective = (1100 * scale) / (1100 * scale - z);
    return {
      x: centerX + x * perspective * 0.9,
      y: centerY + y * perspective * 0.9,
      z,
    };
  };
  const rings = Array.from({ length: ringCount }, (_, i) => {
    const points = Array.from({ length: pointCount }, (_, j) =>
      project(
        (i / ringCount) * Math.PI * 2,
        (j / (pointCount - 1)) * Math.PI * 2,
      ),
    );
    return {
      i,
      depth: points.reduce((sum, point) => sum + point.z, 0) / points.length,
      path: points
        .map(
          (point, j) =>
            (j ? "L" : "M") + point.x.toFixed(1) + "," + point.y.toFixed(1),
        )
        .join(""),
    };
  }).sort((a, b) => a.depth - b.depth);
  return (
    <svg width={width} height={height} viewBox={"0 0 " + width + " " + height}>
      <defs>
        <radialGradient id={"halo-" + width + "-" + height}>
          <stop
            offset="0"
            stopColor={
              emphasis
                ? (style.haloAccent ?? "#373049")
                : (style.halo ?? "#213238")
            }
            stopOpacity="0.48"
          />
          <stop
            offset="1"
            stopColor={theme.background}
            stopOpacity="0"
          />
        </radialGradient>
      </defs>
      <ellipse
        cx={centerX}
        cy={centerY}
        rx={Number(style.ellipseRx ?? width * 0.5)}
        ry={Number(style.ellipseRy ?? height * 0.48)}
        fill={"url(#halo-" + width + "-" + height + ")"}
      />
      {(Array.isArray(style.circleRadii)
        ? style.circleRadii
        : [Math.min(width, height) * 0.37, Math.min(width, height) * 0.46]
      ).map((radius: number) => (
        <circle
          key={radius}
          cx={centerX}
          cy={centerY}
          r={radius}
          fill="none"
          stroke={style.circleColor ?? "#535865"}
          strokeOpacity="0.16"
          strokeDasharray="2 12"
        />
      ))}
      {rings.map(({ i, depth, path }) => {
        const track = palette[Math.floor((i / ringCount) * palette.length)];
        return (
          <path
            key={i}
            d={path}
            fill="none"
            stroke={
              track.level > 0.055 ? track.color : (style.ringColor ?? "#9da7b8")
            }
            strokeOpacity={
              (0.14 + ((depth / scale + 320) / 640) * 0.48) *
              (0.7 + track.level * 0.5)
            }
            strokeWidth={1.1 + track.level * 0.8}
          />
        );
      })}
    </svg>
  );
}
