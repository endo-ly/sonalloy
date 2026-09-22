import { attack, playing, VisualizerInput } from "./types";

export function ScoreMachine({ data, frame }: VisualizerInput) {
  if (
    data.tracks.some(
      (track) => !["melody", "percussion"].includes(track.kind ?? ""),
    )
  )
    throw new Error(
      "ScoreMachine requires every track to declare kind as melody or percussion",
    );
  const t = frame / data.fps,
    tracks = data.tracks.filter((tr) => tr.kind === "melody");
  const percussion = data.tracks.filter((tr) => tr.kind === "percussion");
  if (tracks.length > 5)
    throw new Error("ScoreMachine supports at most 5 melodic tracks");
  if (percussion.length > 4)
    throw new Error("ScoreMachine supports at most 4 percussion tracks");
  const notes = tracks.flatMap((tr) => tr.notes),
    low = notes.length ? Math.min(...notes.map((n) => n.pitch)) - 2 : 48,
    high = notes.length ? Math.max(...notes.map((n) => n.pitch)) + 2 : 72;
  const titleWords = data.title.trim().toUpperCase().split(/\s+/);
  const titleLines =
    titleWords.length > 1
      ? [titleWords.slice(0, -1).join(" "), titleWords.at(-1)!]
      : titleWords;
  const isBlack = (pitch: number) => [1, 3, 6, 8, 10].includes(pitch % 12);
  const whitePitches = Array.from(
    { length: high - low + 1 },
    (_, i) => low + i,
  ).filter((pitch) => !isBlack(pitch));
  const whiteIndex = new Map(
    whitePitches.map((pitch, index) => [pitch, index]),
  );
  const whiteStep = 870 / whitePitches.length;
  const blackWidth = whiteStep * 0.62;
  const blackHeight = 58;
  const keyX = (pitch: number) => {
    const white = whiteIndex.get(pitch);
    if (white !== undefined) return 65 + (white + 0.5) * whiteStep;
    const previousWhite = whiteIndex.get(pitch - 1);
    if (previousWhite !== undefined) {
      return 65 + (previousWhite + 1) * whiteStep;
    }
    const nextWhite = whiteIndex.get(pitch + 1);
    return 65 + (nextWhite ?? 0) * whiteStep;
  };
  const strike = 1280;
  return (
    <>
      <rect width="1000" height="1778" fill="#aebdaf" />
      {titleLines.map((line, index) => (
        <text
          key={line}
          x="57"
          y={245 + index * 137}
          fontSize="148"
          fontWeight="800"
          letterSpacing="-12"
          fill="#202a29"
        >
          {line}
        </text>
      ))}
      <defs>
        <clipPath id="falling-notes">
          <rect x="65" y="160" width="870" height="1120" />
        </clipPath>
      </defs>
      <g>
        {whitePitches.map((pitch) => {
          const active = tracks.find((tr) =>
            playing(tr, t).some((note) => note.pitch === pitch),
          );
          return (
            <g key={pitch}>
              <line
                x1={keyX(pitch)}
                x2={keyX(pitch)}
                y1="160"
                y2={strike}
                stroke="#7d8e85"
                opacity="0.14"
              />
              <rect
                x={65 + whiteIndex.get(pitch)! * whiteStep + 1}
                y={strike}
                width={whiteStep - 2}
                height="90"
                rx="2"
                fill={active ? active.color : "#d2d2c7"}
                stroke="#718178"
                strokeOpacity="0.35"
              />
            </g>
          );
        })}
        {Array.from({ length: high - low + 1 }, (_, i) => low + i)
          .filter(isBlack)
          .map((pitch) => {
            const active = tracks.find((tr) =>
              playing(tr, t).some((note) => note.pitch === pitch),
            );
            return (
              <rect
                key={pitch}
                x={keyX(pitch) - blackWidth / 2}
                y={strike}
                width={blackWidth}
                height={blackHeight}
                rx="3"
                fill={active ? active.color : "#263230"}
                stroke="#17211e"
                strokeWidth="2"
              />
            );
          })}
      </g>
      <g clipPath="url(#falling-notes)">
        {tracks.flatMap((track, i) =>
          track.notes
            .filter((n) => n.start + n.duration > t - 0.3 && n.start < t + 4.2)
            .map((n, j) => {
              const active = n.start <= t && n.start + n.duration > t;
              const width = isBlack(n.pitch)
                ? blackWidth * 0.78
                : whiteStep * 0.72;
              const x = keyX(n.pitch) - width / 2,
                y = strike - (n.start - t) * 200,
                length = Math.max(10, n.duration * 200);
              return (
                <g key={`${i}-${j}`}>
                  <rect
                    x={x + 1}
                    y={y - length}
                    width={width}
                    height={length - 3}
                    rx="3"
                    fill={track.color}
                    opacity={active ? 1 : 0.75}
                    stroke="#202a2920"
                  />
                  <line
                    x1={x + 3}
                    x2={x + width - 3}
                    y1={y - 5}
                    y2={y - 5}
                    stroke="#203c37"
                    opacity="0.5"
                  />
                </g>
              );
            }),
        )}
      </g>
      <line
        x1="65"
        x2="935"
        y1={strike}
        y2={strike}
        stroke="#203b36"
        strokeWidth="3"
      />
      {tracks.flatMap((track, i) =>
        track.notes
          .filter((n) => t >= n.start && t - n.start < 0.35)
          .map((n, j) => {
            const age = (t - n.start) / 0.35;
            return (
              <circle
                key={`${i}-${j}`}
                cx={keyX(n.pitch)}
                cy={strike}
                r={5 + age * 28}
                fill="none"
                stroke={track.color}
                strokeWidth={2 * (1 - age)}
                opacity={1 - age}
              />
            );
          }),
      )}
      {tracks.map((track, i) => (
        <g
          key={track.id}
          transform={`translate(${65 + (i % 3) * 300},${1440 + Math.floor(i / 3) * 70})`}
        >
          <circle
            r={4 + attack(track, t) * 7}
            fill={track.color}
            stroke="#435447"
          />
          <text x="20" y="6" fill="#34443f" fontSize="20">
            {track.name}
          </text>
        </g>
      ))}
      {data.tracks
        .filter((tr) => tr.kind === "percussion")
        .map((track, i) => (
          <g key={track.id} transform={`translate(${65 + i * 224},1640)`}>
            <rect width="190" height="6" fill="#c4c7bb" />
            <rect
              width={190 * attack(track, t, 14)}
              height="6"
              fill="#263b34"
            />
            <text y="36" fill="#56645b" fontSize="17">
              {track.name.replace("Electronic ", "")}
            </text>
          </g>
        ))}
    </>
  );
}
