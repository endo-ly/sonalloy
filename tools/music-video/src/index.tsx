import { registerRoot } from "remotion";
import { InstrumentScoreComposition } from "./compositions/InstrumentScore";
import { VisualizerComposition } from "./compositions/Visualizer";

function RemotionRoot() {
  return (
    <>
      <InstrumentScoreComposition />
      <VisualizerComposition />
    </>
  );
}

registerRoot(RemotionRoot);
