import type { Scene, Track } from "../model/scene";

export type VisualizerInput = {
  data: Scene;
  frame: number;
  width: number;
  height: number;
};
export const level = (track: Track, frame: number) =>
  track.activity?.[frame] ?? 0;
export function attack(track: Track, seconds: number, decay = 9) {
  return track.notes.reduce((peak, n) => {
    const age = seconds - n.start;
    return age >= 0 && age < 1
      ? Math.max(peak, (Math.exp(-age * decay) * n.velocity) / 127)
      : peak;
  }, 0);
}
export const playing = (track: Track, seconds: number) =>
  track.notes.filter(
    (n) => n.start <= seconds && n.start + n.duration > seconds,
  );
