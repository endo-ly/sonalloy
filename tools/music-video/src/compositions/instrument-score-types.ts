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
