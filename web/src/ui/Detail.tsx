import { useState } from "react";
import { newClipAt } from "../state/actions";
import { useStore } from "../state/store";
import { AudioClipView, PianoRoll, StepGrid } from "./ClipEditors";
import { DrumDevice, FxDevice, ReturnsDevice, SynthDevice } from "./Devices";

type Tab = "instrument" | "fx" | "returns";

export function Detail() {
  const sel = useStore((s) => s.selection);
  const t = useStore((s) => s.project.tracks[s.selection.track]);
  const [pad, setPad] = useState(0);
  const [tab, setTab] = useState<Tab>("instrument");
  const clip = t && sel.slot !== null ? t.clips[sel.slot] ?? null : null;
  const ti = sel.track;

  let editor: React.ReactNode;
  if (!t) {
    editor = (
      <p className="empty-note">
        Add a <b>drum rack</b>, <b>synth</b> or <b>audio</b> track with the buttons to the right of the grid, or drop a loop from the browser there.
      </p>
    );
  } else if (clip && sel.slot !== null) {
    if (clip.audio) editor = <AudioClipView ti={ti} slot={sel.slot} clip={clip} />;
    else if (t.kind === "drums") editor = <StepGrid ti={ti} slot={sel.slot} clip={clip} track={t} selectedPad={pad} onSelectPad={setPad} />;
    else editor = <PianoRoll ti={ti} slot={sel.slot} clip={clip} track={t} />;
  } else {
    editor = (
      <div className="empty-note">
        {t.kind === "audio" ? (
          <p style={{ margin: 0 }}>
            Drag a loop from the browser onto one of <b>{t.name}</b>'s slots. Loops follow the project tempo.
          </p>
        ) : (
          <>
            <p style={{ margin: "0 0 10px" }}>
              Select a clip in <b>{t.name}</b> to edit it, or double-click an empty slot to create one. Play the computer keyboard (A–L, W–P for sharps, Z/X for octave) to hear the {t.kind === "drums" ? "pads" : "synth"}.
            </p>
            {sel.slot !== null && (
              <button className="btn primary" onClick={() => newClipAt(ti, sel.slot!)}>
                Create clip in this slot
              </button>
            )}
          </>
        )}
      </div>
    );
  }

  return (
    <section className="detail" aria-label="Clip and device editor">
      <div className="pane">{clip ? editor : (
        <>
          <div className="pane-head"><h3>{t ? `${t.name} · ${sel.slot !== null ? `slot ${sel.slot + 1}` : "no clip selected"}` : "No track"}</h3></div>
          <div className="pane-body">{editor}</div>
        </>
      )}</div>
      <div className="pane">
        <div className="pane-head">
          <div className="seg" role="tablist">
            {(
              [
                ["instrument", t?.kind === "drums" ? "Drum rack" : t?.kind === "audio" ? "Audio" : "Synth"],
                ["fx", "Track FX"],
                ["returns", "Returns"],
              ] as [Tab, string][]
            ).map(([k, l]) => (
              <button key={k} role="tab" aria-selected={tab === k} className={tab === k ? "on" : ""} onClick={() => setTab(k)}>
                {l}
              </button>
            ))}
          </div>
          {t && <h3 className="muted" style={{ fontWeight: 400 }}>{t.name}</h3>}
        </div>
        <div className="pane-body devices">
          {tab === "returns" && <ReturnsDevice />}
          {t && tab === "fx" && <FxDevice ti={ti} t={t} />}
          {t && tab === "instrument" && t.kind === "synth" && <SynthDevice ti={ti} t={t} />}
          {t && tab === "instrument" && t.kind === "drums" && <DrumDevice ti={ti} t={t} selectedPad={pad} onSelectPad={setPad} />}
          {t && tab === "instrument" && t.kind === "audio" && (
            <p className="empty-note">
              Audio tracks play loops and one-shots from the library. Each clip has its own gain and tempo setting in the clip editor; the track's filter, drive, compressor and sidechain are under <b>Track FX</b>.
            </p>
          )}
        </div>
      </div>
    </section>
  );
}
