import { level, playing, VisualizerInput, Track } from "./types";

function flash(track: Track, seconds: number) {
  return track.notes.reduce((peak, note) => {
    const age = seconds - note.start;
    if (age < 0 || age > 0.8) return peak;
    const rise = Math.min(1, age / 0.08);
    return Math.max(peak, (rise * Math.exp(-age * 5) * note.velocity) / 127);
  }, 0);
}

function blend(from: string, to: string, amount: number) {
  return `#${[1, 3, 5]
    .map((offset) =>
      Math.round(
        parseInt(from.slice(offset, offset + 2), 16) * (1 - amount) +
          parseInt(to.slice(offset, offset + 2), 16) * amount,
      )
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

function trackHash(id: string) {
  let hash = 2166136261;
  for (const character of id) {
    hash ^= character.charCodeAt(0);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}

export function ImpactGrid({ data, frame }: VisualizerInput) {
  const t = frame / data.fps;
  const section = data.sections.filter((s) => s.at <= t).length - 1;
  const orders = [
    [5, 0, 2, 1, 6, 3, 7, 4, 8],
    [0, 5, 3, 7, 2, 6, 4, 8, 1],
    [3, 6, 0, 2, 5, 8, 1, 7, 4],
    [4, 1, 5, 0, 3, 2, 8, 6, 7],
  ];
  const order = orders[Math.max(0, section) % orders.length].filter(
    (index) => index < data.tracks.length,
  );
  return (
    <>
      <rect width="1000" height="1778" fill="#161515" />
      {order.map((index, slot) => {
        const tr = data.tracks[index];
        if (!tr) return null;
        const hit = flash(tr, t),
          energy = level(tr, frame);
        const active = playing(tr, t),
          last = tr.notes.filter((note) => note.start <= t).at(-1),
          pitch = last?.pitch ?? 0;
        const shape = trackHash(tr.id) % 4;
        const x = 50 + (slot % 3) * 300,
          y = 225 + Math.floor(slot / 3) * 440;
        const color = tr.color,
          ink = blend(color, "#f4efe4", hit * 0.95);
        return (
          <g key={tr.id} transform={`translate(${x},${y})`}>
            <rect width="286" height="426" rx="3" fill="#242323" />
            <rect
              width="286"
              height="426"
              rx="3"
              fill={color}
              opacity={hit * 0.8}
            />
            <text x="20" y="35" fontSize="15" letterSpacing="2" fill="#bdb6ac">
              {String(index + 1).padStart(2, "0")}
            </text>
            <g transform="translate(143,203)" fill={ink} stroke={ink}>
              {shape === 0 ? (
                <>
                  {Array.from({ length: 5 }, (_, i) => (
                    <rect
                      key={i}
                      x={-100 + i * 42}
                      y={-25 - energy * (35 + (i % 3) * 25)}
                      width="27"
                      height={50 + energy * (70 + (i % 3) * 50)}
                      stroke="none"
                    />
                  ))}
                </>
              ) : shape === 1 ? (
                <>
                  {Array.from({ length: 4 }, (_, i) => (
                    <circle
                      key={i}
                      r={18 + i * 23 + hit * 12}
                      fill={i === 0 ? undefined : "none"}
                      strokeWidth={2 + hit * 8}
                      opacity={0.3 + i * 0.2}
                    />
                  ))}
                </>
              ) : shape === 2 ? (
                <g transform={`rotate(${(pitch % 12) * 15})`}>
                  {Array.from({ length: 8 }, (_, i) => (
                    <rect
                      key={i}
                      x={18 + hit * 22}
                      y="-6"
                      width={45 + energy * 45}
                      height="12"
                      stroke="none"
                      transform={`rotate(${i * 45})`}
                    />
                  ))}
                </g>
              ) : (
                <>
                  {Array.from({ length: 6 }, (_, i) => (
                    <line
                      key={i}
                      x1={-105 + i * 35}
                      y1={70 - hit * 25}
                      x2={-65 + i * 35}
                      y2={-70 + hit * 25}
                      strokeWidth={4 + energy * 14}
                    />
                  ))}
                </>
              )}
            </g>
            <text x="20" y="352" fill={ink} fontSize="25" fontWeight="700">
              {tr.name
                .replace("Electronic ", "")
                .replace("Closed Hi-Hat", "Hi-Hat")
                .replace("Open Hi-Hat", "Open Hat")}
            </text>
            <text x="20" y="392" fill="#aba398" fontSize="14" letterSpacing="2">
              {tr.kind === "percussion"
                ? ""
                : active.length
                  ? active
                      .map(
                        (note) =>
                          [
                            "C",
                            "C♯",
                            "D",
                            "D♯",
                            "E",
                            "F",
                            "F♯",
                            "G",
                            "G♯",
                            "A",
                            "A♯",
                            "B",
                          ][note.pitch % 12],
                      )
                      .join(" · ")
                  : "—"}
            </text>
            <rect
              x="220"
              y={390 - energy * 40}
              width="42"
              height={2 + energy * 40}
              fill={ink}
            />
          </g>
        );
      })}
      <text x="53" y="1665" fill="#e4ded2" fontSize="31" letterSpacing="-1">
        {data.title.toUpperCase()}
      </text>
    </>
  );
}
