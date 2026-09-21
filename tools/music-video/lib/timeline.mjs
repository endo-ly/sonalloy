import { readFileSync } from "node:fs";

function finite(value, field, minimum = 0) {
  if (!Number.isFinite(value) || value < minimum)
    throw new Error(`${field}: invalid value`);
  return value;
}

export function patternNotes(pattern) {
  const resolution = finite(pattern.ticks_per_beat, "ticks_per_beat", 1);
  if (!Number.isInteger(resolution))
    throw new Error("ticks_per_beat: expected integer");
  const tempos = pattern.tempo_changes;
  if (!Array.isArray(tempos) || !tempos.length || tempos[0].tick !== 0)
    throw new Error("tempo_changes: must start at tick 0");
  let seconds = 0;
  const map = tempos.map((tempo, i) => {
    finite(tempo.tick, `tempo_changes[${i}].tick`);
    finite(tempo.bpm, `tempo_changes[${i}].bpm`, Number.MIN_VALUE);
    if (i) {
      if (tempo.tick <= tempos[i - 1].tick)
        throw new Error("tempo_changes: ticks must increase");
      seconds +=
        (((tempo.tick - tempos[i - 1].tick) / resolution) * 60) /
        tempos[i - 1].bpm;
    }
    return { ...tempo, seconds };
  });
  function at(tick) {
    let segment = map[0];
    for (const next of map) {
      if (next.tick > tick) break;
      segment = next;
    }
    return (
      segment.seconds +
      (((tick - segment.tick) / resolution) * 60) / segment.bpm
    );
  }
  if (!Array.isArray(pattern.events)) throw new Error("events: expected array");
  return pattern.events
    .filter((e) => e.type === "note")
    .map((e, i) => {
      finite(e.tick, `events[${i}].tick`);
      finite(e.duration_ticks, `events[${i}].duration_ticks`, Number.MIN_VALUE);
      const start = at(e.tick);
      return {
        start,
        duration: at(e.tick + e.duration_ticks) - start,
        pitch: e.note,
        velocity: e.velocity,
      };
    });
}

export function clipNotes(notes, clip) {
  if (!Array.isArray(notes)) throw new Error("notes: expected array");
  return notes
    .map((n, i) => {
      finite(n.start, `notes[${i}].start`);
      finite(n.duration, `notes[${i}].duration`, Number.MIN_VALUE);
      finite(n.pitch, `notes[${i}].pitch`);
      finite(n.velocity, `notes[${i}].velocity`, 1);
      if (
        !Number.isInteger(n.pitch) ||
        n.pitch > 127 ||
        !Number.isInteger(n.velocity) ||
        n.velocity > 127
      )
        throw new Error(`notes[${i}]: pitch/velocity outside MIDI range`);
      return { ...n, start: n.start - clip.startSeconds };
    })
    .filter((n) => n.start < clip.durationSeconds && n.start + n.duration > 0)
    .sort((a, b) => a.start - b.start);
}

export function readNotes(track, clip) {
  const input = track.notes;
  if (!input) return [];
  const json = JSON.parse(readFileSync(input, "utf8"));
  return clipNotes(Array.isArray(json) ? json : patternNotes(json), clip);
}
