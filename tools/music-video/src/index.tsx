import { registerRoot } from "remotion";
import { InstrumentScoreComposition } from "./compositions/InstrumentScore";
import { MusicVideoComposition } from "./compositions/MusicVideo";
import { VisualizerComposition } from "./compositions/Visualizer";

function RemotionRoot() {
  return (
    <>
      <MusicVideoComposition />
      <InstrumentScoreComposition />
      <VisualizerComposition />
    </>
  );
}

registerRoot(RemotionRoot);
