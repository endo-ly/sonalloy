import { Sculpture } from "./Sculpture";
import { attack, level, VisualizerInput } from "./types";

export function Resonance({ data, frame }: VisualizerInput) {
  const t = frame / data.fps;
  const melodic = data.tracks.filter((track) => track.kind === "melody");
  const drums = data.tracks.filter((track) => track.kind === "percussion");
  const sculptureData = {
    ...data,
    visual: { theme: { accent: "#88ded3", background: "#0c1018" } },
  };
  return (
    <>
      <rect width="1000" height="1778" fill="#0c1018" />
      <g transform="translate(0,180)">
        <Sculpture
          data={sculptureData}
          frame={frame}
          width={1000}
          height={800}
          style={{
            centerX: 0.5,
            centerY: 0.48,
            ringCount: 90,
            pointCount: 101,
            rotationSpeed: 0.14,
          }}
        />
      </g>
      <g transform="translate(65,980)">
        {data.bands[frame].map((v, i) => (
          <rect
            key={i}
            x={i * 13.7}
            y={-v * 90}
            width="6"
            height={v * 90}
            rx="3"
            fill={melodic[Math.floor((i / 64) * melodic.length)].color}
            opacity={0.3 + v * 0.6}
          />
        ))}
      </g>
      <defs>
        <clipPath id="score-window">
          <rect x="300" y="1080" width="635" height="420" />
        </clipPath>
      </defs>
      {melodic.map((track, i) => {
        const y = 1090 + i * 84,
          energy = level(track, frame);
        const pitches = track.notes.map((n) => n.pitch),
          low = Math.min(...pitches),
          high = Math.max(...pitches);
        return (
          <g key={track.id}>
            <line x1="65" x2="935" y1={y + 67} y2={y + 67} stroke="#26303d" />
            <circle
              cx="74"
              cy={y + 25}
              r={3 + attack(track, t) * 5}
              fill={track.color}
            />
            <text
              x="96"
              y={y + 31}
              fontSize="21"
              fill={energy > 0.04 ? track.color : "#8491a3"}
            >
              {track.name}
            </text>
            <rect
              x="96"
              y={y + 45}
              width={energy * 152}
              height="2"
              fill={track.color}
            />
            <g clipPath="url(#score-window)">
              {track.notes
                .filter(
                  (n) => n.start + n.duration > t - 0.4 && n.start < t + 3.6,
                )
                .map((n, j) => {
                  const active = n.start <= t && n.start + n.duration > t;
                  return (
                    <rect
                      key={j}
                      x={340 + (n.start - t) * 166}
                      y={
                        y +
                        48 -
                        ((n.pitch - low) / Math.max(1, high - low)) * 38
                      }
                      width={Math.max(4, n.duration * 166 - 2)}
                      height={active ? 7 : 4}
                      rx="2"
                      fill={track.color}
                      opacity={active ? 1 : n.start < t ? 0.3 : 0.6}
                    />
                  );
                })}
            </g>
          </g>
        );
      })}
      <line
        x1="340"
        x2="340"
        y1="1080"
        y2="1495"
        stroke="#ffffff"
        opacity="0.5"
      />
      {drums.map((track, i) => {
        const hit = attack(track, t, 15),
          x = 65 + i * 220;
        return (
          <g key={track.id}>
            <rect
              x={x}
              y="1550"
              width="205"
              height="103"
              rx="12"
              fill={track.color}
              opacity={0.025 + hit * 0.16}
            />
            <circle
              cx={x + 24}
              cy="1580"
              r={4 + hit * 8}
              fill={track.color}
              opacity={0.35 + hit * 0.65}
            />
            <text x={x + 18} y="1630" fontSize="17" fill="#a9b4c3">
              {track.name
                .replace("Electronic ", "")
                .replace("Closed Hi-", "")
                .replace("Open Hi-", "Open ")}
            </text>
          </g>
        );
      })}
    </>
  );
}
