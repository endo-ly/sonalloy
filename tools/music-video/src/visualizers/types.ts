export type Note = {
  start: number;
  duration: number;
  pitch: number;
  velocity: number;
};
export type Track = {
  id: string;
  name: string;
  category?: string;
  kind: string;
  color: string;
  energy: number[] | null;
  notes: Note[];
  scopes?: number[][];
};
export type Scene = {
  title: string;
  audio: string;
  fps: number;
  frames: number;
  width: number;
  height: number;
  bands: number[][];
  energy: number[];
  tracks: Track[];
  sections: { at: number; emphasis: boolean }[];
};
export type VisualizerInput = {
  data: Scene;
  frame: number;
  width: number;
  height: number;
};
export const level = (track: Track, frame: number) =>
  track.energy?.[frame] ?? 0;
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
