import { Resonance } from "./Resonance";
import { ScoreMachine } from "./ScoreMachine";
import { ImpactGrid } from "./ImpactGrid";
import { PhaseGarden } from "./PhaseGarden";

export { Resonance, ScoreMachine, ImpactGrid, PhaseGarden };
export { Sculpture } from "./Sculpture";
export const visualizers = {
  resonance: Resonance,
  "score-machine": ScoreMachine,
  "impact-grid": ImpactGrid,
  "phase-garden": PhaseGarden,
} as const;
export type { VisualizerInput } from "./types";
export type VisualizerName = keyof typeof visualizers;
