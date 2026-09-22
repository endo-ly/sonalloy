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
  kind?: string;
  tags?: string[];
  color: string;
  activity: number[];
  notes: Note[];
  scopes?: number[][];
};
export type Section = {
  at: number;
  label: string;
  color: string;
  emphasis: boolean;
};
export type VisualTheme = {
  background: string;
  foreground: string;
  muted: string;
  line: string;
  accent: string;
  fontFamily: string;
  monoFamily: string;
  gridOpacity: number;
  cornerRadius: number;
};
export type VisualElement = {
  id: string;
  type: string;
  x: number;
  y: number;
  width: number;
  height: number;
  visible: boolean;
  bind: string | null;
  text: string | null;
  style: Record<string, any>;
};
export type VisualConfig = {
  canvas: { width: number; height: number };
  delivery: { width: number; height: number };
  theme: VisualTheme;
  elements: VisualElement[];
};
export type Scene = {
  title: string;
  audio: string;
  fps: number;
  frames: number;
  duration: number;
  fadeOut: number;
  width: number;
  height: number;
  bands: number[][];
  energy: number[];
  tracks: Track[];
  sections: Section[];
  presentation?: Record<string, string>;
  visual?: VisualConfig;
};
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
