import {
  AbsoluteFill,
  Audio,
  Composition,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import {
  Resonance,
  ScoreMachine,
  ImpactGrid,
  PhaseGarden,
  Scene,
} from "../visualizers";
const visuals = {
  resonance: Resonance,
  "score-machine": ScoreMachine,
  "impact-grid": ImpactGrid,
  "phase-garden": PhaseGarden,
};
type Props = {
  scenePath: string;
  variant: keyof typeof visuals;
  data: Scene | null;
};
function Film({ data, variant }: Props) {
  const frame = useCurrentFrame();
  const { width, height } = useVideoConfig();
  if (!data) throw new Error("Missing prepared score and audio");
  const Artwork = visuals[variant];
  return (
    <AbsoluteFill style={{ background: "#090d10" }}>
      <Audio src={staticFile(data.audio)} />
      <svg
        width={width}
        height={height}
        viewBox="0 0 1000 1778"
        style={{ fontFamily: "Arial, Helvetica, sans-serif" }}
      >
        <Artwork data={data} frame={frame} width={1000} height={1778} />
      </svg>
    </AbsoluteFill>
  );
}
export function VisualizerComposition() {
  return (
    <Composition
      id="Visualizer"
      component={Film}
      width={1080}
      height={1920}
      fps={30}
      durationInFrames={900}
      defaultProps={{ scenePath: "", variant: "resonance", data: null } as Props}
      calculateMetadata={async ({ props, abortSignal }) => {
        if (!Object.hasOwn(visuals, props.variant))
          throw new Error("Unknown visualizer");
        const response = await fetch(staticFile(props.scenePath), {
          signal: abortSignal,
        });
        if (!response.ok) throw new Error("Cannot load " + props.scenePath);
        const data: Scene = await response.json();
        return {
          width: data.width,
          height: data.height,
          fps: data.fps,
          durationInFrames: data.frames,
          props: { ...props, data },
        };
      }}
    />
  );
}
