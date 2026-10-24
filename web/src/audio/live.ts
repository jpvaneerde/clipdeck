// Live playing: computer keyboard and Web MIDI into the selected track, with
// optional overdub recording into the selected (playing) clip.

import { engine } from "./host";
import { store, updateTrack } from "../state/store";
import type { Note } from "../state/types";

const held = new Map<number, { track: number; start: number | null; vel: number }>();
const QUANT = 0.25;

function recordingTarget(track: number): { slot: number } | null {
  const s = store.get();
  if (!s.recordArmed) return null;
  const slot = s.selection.slot;
  if (slot === null || s.selection.track !== track) return null;
  if (engine.transport.slots[track] !== slot) return null;
  const clip = s.project.tracks[track]?.clips[slot];
  if (!clip || clip.audio) return null;
  return { slot };
}

export function noteOn(pitch: number, vel = 0.85) {
  const track = store.get().selection.track;
  const t = store.get().project.tracks[track];
  if (!t || t.kind === "audio") return;
  if (held.has(pitch)) return;
  engine.cmd("note_on", track, pitch, vel);
  const target = recordingTarget(track);
  held.set(pitch, { track, start: target ? engine.trackBeat(track).local : null, vel });
}

export function noteOff(pitch: number) {
  const h = held.get(pitch);
  if (!h) return;
  held.delete(pitch);
  engine.cmd("note_off", h.track, pitch);
  if (h.start === null) return;
  const target = recordingTarget(h.track);
  if (!target) return;
  const { local, length } = engine.trackBeat(h.track);
  if (length <= 0) return;
  let start = Math.round(h.start / QUANT) * QUANT;
  if (start >= length) start -= length;
  let dur = local - h.start;
  if (dur < 0) dur += length;
  const len = Math.max(QUANT, Math.round(dur / QUANT) * QUANT);
  const note: Note = { s: start, l: Math.min(len, length), p: pitch, v: h.vel };
  store.edit(
    (p) =>
      updateTrack(p, h.track, (t) => {
        const clip = t.clips[target.slot];
        if (!clip) return t;
        // Replace a note already at this position and pitch (overdub).
        const notes = clip.notes.filter((n) => !(n.p === note.p && Math.abs(n.s - note.s) < 1e-6));
        const clips = t.clips.slice();
        clips[target.slot] = { ...clip, notes: [...notes, note] };
        return { ...t, clips };
      }),
    undefined,
  );
}

export function allNotesOff() {
  for (const p of [...held.keys()]) noteOff(p);
}

// ------------------------------------------------------------ computer keyboard

// Two rows like a piano: white keys on A S D F..., black keys on W E T Y U...
const KEYMAP: Record<string, number> = {
  KeyA: 0, KeyW: 1, KeyS: 2, KeyE: 3, KeyD: 4, KeyF: 5, KeyT: 6, KeyG: 7, KeyY: 8, KeyH: 9, KeyU: 10, KeyJ: 11,
  KeyK: 12, KeyO: 13, KeyL: 14, KeyP: 15, Semicolon: 16,
};

/** Drum tracks map the same keys to pads 1..16 (MIDI 36..51). */
export function keyToPitch(code: string): number | null {
  const off = KEYMAP[code];
  if (off === undefined) return null;
  const s = store.get();
  const t = s.project.tracks[s.selection.track];
  if (t?.kind === "drums") return off < 16 ? 36 + off : null;
  return 12 * (s.octave + 1) + off;
}

// ------------------------------------------------------------ Web MIDI

export async function connectMidi(): Promise<string[]> {
  const nav = navigator as Navigator & { requestMIDIAccess?: () => Promise<MIDIAccess> };
  if (!nav.requestMIDIAccess) throw new Error("This browser has no Web MIDI support. Chrome and Edge do.");
  const access = await nav.requestMIDIAccess();
  const names: string[] = [];
  const attach = () => {
    names.length = 0;
    access.inputs.forEach((input) => {
      names.push(input.name ?? "MIDI input");
      input.onmidimessage = (e: MIDIMessageEvent) => {
        const d = e.data;
        if (!d || d.length < 3) return;
        const status = d[0] & 0xf0;
        if (status === 0x90 && d[2] > 0) noteOn(d[1], d[2] / 127);
        else if (status === 0x80 || (status === 0x90 && d[2] === 0)) noteOff(d[1]);
      };
    });
  };
  attach();
  access.onstatechange = attach;
  return names;
}
