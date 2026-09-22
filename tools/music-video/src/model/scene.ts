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
};
