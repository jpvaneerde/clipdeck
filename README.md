# Clipdeck

A clip-launching DAW in the browser, in the spirit of Ableton Live's Session View, with its own
royalty-free sample library. The audio engine is Rust compiled to WebAssembly and runs inside an
AudioWorklet in your browser; the server is Rust too and runs in Kubernetes. Exports are rendered on
the server by the same engine, so the file sounds exactly like the session.

## What it does

- **Session grid**: up to 16 tracks × 16 scenes of looping clips. Launch clips or whole scenes;
  launches snap to the next beat, bar or 2/4 bars. Stop buttons per track, "stop all", scene names,
  duplicate scenes, drag clips between slots (Alt-drag copies).
- **Three track types**
  - *Drum rack*: 16 pads with gain, pitch, pan, decay and choke groups; step sequencer editor.
  - *Synth*: polyphonic subtractive synth (2 band-limited oscillators, sub, noise, resonant filter
    with its own envelope, LFO, glide, mono/poly, stereo width, drive) with 18 presets; piano roll.
  - *Audio*: plays loops and one-shots from the library, locked to the project tempo.
- **Mixer and effects**: volume, pan, mute, solo, two sends per track; per-track filter, drive,
  compressor and sidechain ducking from any other track; reverb and ping-pong delay returns;
  master limiter; meters everywhere.
- **Play and record**: computer keyboard (A–L white keys, W–P black keys, Z/X octave) and Web MIDI
  keyboards play the selected track. With record armed, notes overdub into the selected playing
  clip, quantized to 1/16.
- **Sample browser**: full-text search with any-word fallback, filters for category, genre, key
  (with relative major/minor), tempo near the project, kit, favorites, and your own uploads.
  Click to preview, drag onto the grid or a drum pad, double-click to add.
- **Projects**: autosave to the server, a project list, undo/redo, save/open as JSON files.
- **Export**: arrange scenes into a song (scene + bars per section) and render 16/24-bit WAV at
  44.1/48/96 kHz.

## The sample library

`clipdeck-samplegen` synthesizes the library from scratch, so everything is yours to use. A full run
makes about **2,800 samples (1.7 GB)** in a minute or two:

| Category | Count | What |
|---|---|---|
| Drum one-shots | ~1,270 | each kit's kick, snare, clap, hats, percussion, shaker, rim (plus toms and cymbals), and 48 tuned 808s |
| Synth hits | 120 | chord stabs, keys, brass, plucks, bells, bass hits, tagged with key |
| Drum loops | 360 | full, tops (no kick) and fill variations |
| Bass | ~190 | sub, pluck, acid, reese, 808 slides, organ and disco bass lines |
| Chords | 240 | pads, keys, stabs, brass, supersaws in 9 rhythms |
| Hooks | 240 | melodic hooks (lead, bell, pluck, flute, chip…) |
| Arps | 120 | arpeggios that follow each kit's progression |
| Vocals | ~180 | formant-synthesized vocal chops and vocal hooks |
| FX | 100 | risers, downlifters, impacts, sweeps, reverse crashes, zaps |

It is organized as **construction kits**: 12 genres (house, deep house, techno, hip-hop, trap,
drum & bass, lo-fi, UK garage, afro house, reggaeton, nu-disco, synthwave) × 10 kits. Every loop in a
kit shares tempo, key and chord progression, so loops from one kit always fit together. Loops are
rendered as seamless cycles (tails wrap around, the seam is crossfaded) and tagged with genre, BPM,
key, bars, mood and sound descriptors. `--scale 2` doubles everything; `--seed` gives a different
library.

**Your own samples**: drop WAV/MP3/FLAC/OGG/M4A files onto the browser, or copy whole packs into
`/data/user/` on the volume and press *Rescan folder*. Tempo and key are read from file names
("Deep_Chords_124bpm_Am.wav"), folder names become tags.

## Layout

```
crates/engine        DSP engine, no dependencies (synth, drum rack, sampler, FX, transport)
crates/engine-wasm   C-ABI wrapper → engine.wasm for the AudioWorklet (imports nothing)
crates/model         project file format + "apply to engine" + arrangement rendering
crates/samplegen     the sample library generator
crates/server        axum server: static UI, SQLite/FTS5 sample index, uploads, projects, export
web/                 React + TypeScript UI (Vite); src/audio/worklet.ts hosts the engine
deploy/kubernetes    kustomize manifests; deploy/argocd has the Argo CD Application
```

## Run locally

Needs Rust (stable) and Node 22.

```sh
make wasm                 # engine → web/public/engine.wasm
make library SCALE=0.1    # a quick 350-sample library in ./data/library (drop SCALE for the full one)
make dev                  # server on :8080, Vite with hot reload on http://localhost:5173
```

`make test` runs the Rust tests (engine timing, quantized launches, tempo sync, mixer bounds, ABI,
database, file-name parsing) and the TypeScript type check.

## Deploy to Kubernetes

1. Push this repo to `github.com/jpvaneerde/clipdeck`. The workflow in `.github/workflows/build.yml`
   tests, then builds `ghcr.io/jpvaneerde/clipdeck:latest` (amd64) on every push to `main`.
2. Make the GHCR package public, or add an image pull secret to the namespace.
3. Edit `deploy/kubernetes/ingress.yaml` to match your ingress class and TLS (it uses
   `clipdeck.intherapy.eu`).
4. Copy `deploy/argocd/clipdeck.yaml` into your Argo CD repo.

On first start an init container generates the library onto the 10 Gi volume (a minute or two);
later starts skip it. The server indexes it in the background and is ready immediately.

**There is no login.** Anyone who can reach the URL can upload samples and change projects, so put it
behind your SSO or forward-auth (for example oauth2-proxy with Entra ID) or keep it on the LAN.

Environment: `CLIPDECK_DATA` (default `/data`), `CLIPDECK_LIBRARY` (default `$CLIPDECK_DATA/library`),
`CLIPDECK_WEB`, `PORT`. CLI: `clipdeck-server index [--force]`,
`clipdeck-server render project.json out.wav [--bars N]`.

## How the audio works

The browser loads `engine.wasm` into an AudioWorklet. The UI keeps the project in an immutable
store; a sync layer diffs each change and sends only what changed (a knob turn is one message,
a clip edit is one array). Samples are fetched, decoded by the browser and copied into the engine's
memory. The engine reports transport, playheads and meters back about 40 times a second.

Inside the engine every 128-frame block is split at sample-accurate event points (note starts,
quantized clip launches), instruments render between them, then each track runs filter → drive →
compressor → sidechain duck → fader/pan → sends. Audio clips are re-pitched to the project tempo
(like Live's "Re-Pitch" warp mode: speed and pitch change together). A busy 12-track session renders
at about 11× real time natively, so a laptop has plenty of headroom.

## API

| Method | Path | |
|---|---|---|
| GET | `/api/samples?q=&category=&genre=&key=&compatible=&bpm=&kit=&favorites=&source=&limit=&offset=` | search |
| GET | `/api/facets` (same filters) | counts per category, genre, key, kit |
| GET | `/api/audio/{id}` | audio file, with byte ranges |
| GET/PATCH/DELETE | `/api/sample?id=` | details, edit metadata, delete an upload |
| POST | `/api/favorite`, `/api/upload` (multipart), `/api/rescan` | |
| GET/POST | `/api/projects`; GET/PUT/DELETE `/api/projects/{id}` | |
| POST | `/api/render?bits=&sampleRate=&tail=` (project JSON body) | WAV |
| GET | `/api/status`, `/healthz` | |

## Limits

- Audio clips re-pitch to fit the tempo; there is no time-stretching that keeps pitch.
- No audio recording from a microphone and no arrangement timeline editing beyond the export's
  scene chain.
- One replica (SQLite on a ReadWriteOnce volume).
