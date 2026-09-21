import React from "react";
import {
  AbsoluteFill,
  Audio,
  Composition,
  interpolate,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";

export type MusicVideoProps = {
  audio?: string;
  eyebrow?: string;
  title: string;
  subtitle?: string;
  accent?: string;
  background?: string;
  durationInSeconds?: number;
};

const defaultProps: MusicVideoProps = {
  title: "Music Video",
  subtitle: "A Remotion composition",
  eyebrow: "SONALLOY",
  accent: "#6de7ff",
  background: "#090a12",
  durationInSeconds: 30,
};

const clamp = (value: number) => Math.min(1, Math.max(0, value));

export const MusicVideo: React.FC<MusicVideoProps> = ({
  audio,
  eyebrow = defaultProps.eyebrow,
  title,
  subtitle = defaultProps.subtitle,
  accent = defaultProps.accent,
  background = defaultProps.background,
}) => {
  const frame = useCurrentFrame();
  const { durationInFrames, fps } = useVideoConfig();
  const progress = frame / Math.max(1, durationInFrames - 1);
  const intro = interpolate(frame, [0, 24], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  const outro = interpolate(
    frame,
    [Math.max(0, durationInFrames - 24), durationInFrames],
    [1, 0],
    { extrapolateLeft: "clamp", extrapolateRight: "clamp" },
  );
  const opacity = intro * outro;
  const pulse = 1 + Math.sin(frame / 10) * 0.025;
  const rotation = frame * 0.35;

  return (
    <AbsoluteFill
      style={{
        background,
        color: "#f6f7fb",
        fontFamily: "Arial, Helvetica, sans-serif",
        overflow: "hidden",
        opacity,
      }}
    >
      {audio ? <Audio src={staticFile(audio)} /> : null}

      <AbsoluteFill
        style={{
          background: `radial-gradient(circle at 68% 45%, ${accent}26 0%, transparent 38%), radial-gradient(circle at 22% 88%, ${accent}14 0%, transparent 32%)`,
        }}
      />

      <div
        style={{
          position: "absolute",
          inset: 0,
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          padding: "64px 80px 56px",
        }}
      >
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            color: `${accent}cc`,
            fontSize: 22,
            letterSpacing: 5,
            fontWeight: 700,
          }}
        >
          <span>{eyebrow}</span>
          <span>{String(Math.round(progress * 100)).padStart(3, "0")} %</span>
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 110 }}>
          <div style={{ maxWidth: 880 }}>
            <div
              style={{
                color: `${accent}e6`,
                fontSize: 24,
                letterSpacing: 3,
                marginBottom: 22,
                textTransform: "uppercase",
              }}
            >
              {subtitle}
            </div>
            <h1
              style={{
                fontSize: 116,
                lineHeight: 0.96,
                letterSpacing: -5,
                margin: 0,
                fontWeight: 800,
              }}
            >
              {title}
            </h1>
          </div>

          <div
            style={{
              width: 390,
              height: 390,
              borderRadius: "50%",
              border: `1px solid ${accent}70`,
              transform: `scale(${pulse}) rotate(${rotation}deg)`,
              boxShadow: `0 0 90px ${accent}20, inset 0 0 70px ${accent}16`,
              position: "relative",
              flexShrink: 0,
            }}
          >
            {[0, 1, 2, 3].map((index) => (
              <div
                key={index}
                style={{
                  position: "absolute",
                  inset: 30 + index * 28,
                  borderRadius: "50%",
                  border: `1px solid ${accent}${index === 0 ? "90" : "45"}`,
                  transform: `rotate(${index * 23}deg) scale(${1 + Math.sin(frame / (14 + index * 3) + index) * 0.05})`,
                }}
              />
            ))}
            <div
              style={{
                position: "absolute",
                left: "50%",
                top: "50%",
                width: 8,
                height: 8,
                borderRadius: "50%",
                background: accent,
                boxShadow: `0 0 28px 8px ${accent}90`,
                transform: "translate(-50%, -50%)",
              }}
            />
          </div>
        </div>

        <div>
          <div
            style={{
              height: 3,
              width: "100%",
              background: `${accent}26`,
              overflow: "hidden",
            }}
          >
            <div
              style={{
                height: "100%",
                width: `${clamp(progress) * 100}%`,
                background: accent,
                boxShadow: `0 0 18px ${accent}`,
              }}
            />
          </div>
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              marginTop: 18,
              color: "#a5a7b4",
              fontSize: 18,
              letterSpacing: 2,
            }}
          >
            <span>REMOTION / AUDIO VISUAL</span>
            <span>{String(Math.floor(frame / fps)).padStart(2, "0")}s</span>
          </div>
        </div>
      </div>
    </AbsoluteFill>
  );
};

export const MusicVideoComposition: React.FC = () => (
  <Composition
    id="MusicVideo"
    component={MusicVideo}
    width={1920}
    height={1080}
    fps={30}
    durationInFrames={900}
    defaultProps={defaultProps}
    calculateMetadata={({ props }) => {
      const seconds =
        typeof props.durationInSeconds === "number" &&
        Number.isFinite(props.durationInSeconds) &&
        props.durationInSeconds > 0
          ? props.durationInSeconds
          : 30;
      return { durationInFrames: Math.max(1, Math.ceil(seconds * 30)) };
    }}
  />
);
