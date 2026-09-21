import path from "node:path";
import { levels, samples, scopes } from "./audio.mjs";
import { readNotes } from "./timeline.mjs";

export function prepareTracks(
  tracks,
  directory,
  clip,
  frames,
  fps,
  { includeScopes = false } = {},
) {
  return tracks.map((track) => {
    const resolved = { ...track };
    for (const field of ["audio", "pattern", "notes"]) {
      if (resolved[field])
        resolved[field] = path.resolve(directory, resolved[field]);
    }
    if (!resolved.audio) throw new Error(`track ${track.id} needs stem audio`);
    const pcm = samples(resolved.audio, clip);
    const energy = levels(pcm, frames, fps);
    const peak = Math.max(0.001, ...energy);
    const prepared = {
      id: track.id,
      name: track.name,
      category: track.category,
      kind: track.kind,
      color: track.color,
      notes: readNotes(resolved, clip),
      energy: energy.map((v) => Number((v / peak).toFixed(4))),
    };
    if (includeScopes) prepared.scopes = scopes(pcm, frames, fps);
    return prepared;
  });
}
