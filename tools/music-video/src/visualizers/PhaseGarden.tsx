import { level, attack, VisualizerInput } from "./types";

export function PhaseGarden({ data, frame }: VisualizerInput) {
  const t = frame / data.fps,
    voices = data.tracks.filter((tr) => tr.kind === "melody");
  const kick = data.tracks.find((tr) => tr.tags?.includes("kick")),
    pulse = kick ? attack(kick, t, 12) : 0;
  return (
    <>
      <rect width="1000" height="1778" fill="#080e11" />
      <defs>
        <radialGradient id="phase-glow">
          <stop stopColor="#1b454c" stopOpacity={0.18 + pulse * 0.18} />
          <stop offset="1" stopColor="#080e11" stopOpacity="0" />
        </radialGradient>
      </defs>
      <ellipse cx="500" cy="810" rx="700" ry="720" fill="url(#phase-glow)" />
      {voices.map((track, i) => {
        const energy = level(track, frame),
          angle = (i / voices.length) * Math.PI * 2 - Math.PI / 2;
        const cx = 500,
          cy = 780;
        return (
          <g key={track.id}>
            {Array.from({ length: 40 }, (_, trail) => {
              const f = Math.max(0, frame - trail),
                samples = track.scopes?.[f] ?? [];
              const scale =
                (100 + 320 * Math.sqrt(level(track, f))) /
                Math.max(0.05, ...samples.map(Math.abs));
              const points = samples
                .slice(0, 130)
                .map((v, j) => {
                  const delayed = samples[(j + 23) % samples.length] ?? 0;
                  const x = v * scale,
                    y = delayed * scale;
                  return `${j ? "L" : "M"}${(cx + x * Math.cos(angle) - y * Math.sin(angle)).toFixed(1)},${(cy + x * Math.sin(angle) + y * Math.cos(angle)).toFixed(1)}`;
                })
                .join("");
              return (
                <path
                  key={trail}
                  d={points}
                  fill="none"
                  stroke={track.color}
                  strokeWidth={trail === 0 ? 1.9 : 0.8}
                  opacity={(1 - trail / 40) * (trail === 0 ? 0.95 : 0.1)}
                />
              );
            })}
            <circle cx={cx} cy={cy} r={1.5 + energy * 3} fill={track.color} />
          </g>
        );
      })}
      {kick?.notes
        .filter((n) => t >= n.start && t - n.start < 0.45)
        .map((n, i) => {
          const age = (t - n.start) / 0.45;
          return (
            <ellipse
              key={i}
              cx="500"
              cy="815"
              rx={80 + age * 400}
              ry={80 + age * 430}
              fill="none"
              stroke="#8cbbbe"
              strokeWidth="1"
              opacity={(1 - age) * 0.3}
            />
          );
        })}
      {voices.map((track, i) => {
        const y = 1290 + i * 65,
          samples = track.scopes?.[frame] ?? [];
        const d = samples
          .map((v, j) => `${j ? "L" : "M"}${280 + j * 4},${y - v * 30}`)
          .join("");
        return (
          <g key={track.id}>
            <text
              x="60"
              y={y + 5}
              fontSize="19"
              fill={level(track, frame) > 0.04 ? track.color : "#526469"}
            >
              {track.name}
            </text>
            <line x1="280" x2="918" y1={y} y2={y} stroke="#1d2d33" />
            <path d={d} stroke={track.color} strokeWidth="1.4" fill="none" />
          </g>
        );
      })}
      {data.tracks
        .filter((tr) => tr.kind === "percussion")
        .map((track, i) => (
          <g key={track.id}>
            <circle
              cx={65 + i * 230}
              cy="1680"
              r={3 + attack(track, t) * 10}
              fill={track.color}
            />
            <text x={85 + i * 230} y="1686" fontSize="15" fill="#6b858a">
              {track.name
                .replace("Electronic ", "")
                .replace("Closed Hi-", "")
                .replace("Open Hi-", "Open ")}
            </text>
          </g>
        ))}
    </>
  );
}
