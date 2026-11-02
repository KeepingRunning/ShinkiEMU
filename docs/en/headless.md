---
title: Headless execution
description: Windowless core acceptance, replay, baseline comparisons and Manager UI screenshots.
---

# Headless execution and Manager screenshots

The shared runner uses Manager's C ABI v3 loader, native session owner and immutable CPU frame leases. Scripts use the existing `roukaku.user_input_sequence.v1` messages from `roukaku-headless-protocol` at the workspace's pinned RoukakuEngine revision. No engine checkout changes are required.

## Build

```powershell
cargo build -p shinki-emu-headless
cargo test -p shinki-emu-headless
cargo clippy -p shinki-emu-headless --all-targets -- -D warnings
```

`--no-default-features` builds the core runner without the Manager UI. `--features static-cmvs` embeds CMVS under `shinki.emu.cmvs`; `--features static-siglus` embeds the same pinned Siglus core used by Manager under `siglus_rs`. Manager shares the runner through `shinki-emu-manager headless ...` and `shinki-emu-manager headless-ui ...`; its embedded core IDs come from the same `static_cores::STATIC_CORES` inventory used by the graphical owner. The library's `run_cli(arguments, STATIC_CORES)` entry accepts any host's linked inventory.

Build Manager with `cargo build -p shinki-emu-manager --features static-cmvs` to register `shinki.emu.cmvs` in both its GUI owner and `shinki-emu-manager headless --core shinki.emu.cmvs ...`. Enabling the standalone runner's feature alone does not register CMVS in Manager. Desktop builds without this optional feature keep their dynamic-core discovery.

## Core acceptance and captures

```powershell
shinki-emu-headless run --core core-package.zip --game-dir C:/Games/MyGame --frames 180 --output artifacts/acceptance
shinki-emu-headless run --core cores/shinki_emu_rfvp --game-dir C:/Games/MyGame --duration-ms 5000 --capture-frame 1,60,180 --capture-ms 2500 --output artifacts/capture
shinki-emu-headless run --core core-package.zip --game-dir C:/Games/MyGame --frames 180 --output artifacts/comparison --baseline artifacts/capture
shinki-emu-headless compare --actual artifacts/comparison/frame-000180.png --baseline artifacts/capture/frame-000180.png --max-channel-delta 4 --changed-pixel-ratio 0.001
```

`--core` accepts a native library, unpacked core directory, published ZIP or a linked static core ID. ZIP dependencies remain beside the extracted library, as in Manager. Only normal ABI query/descriptor/probe/open checks run; there is no release-provenance audit.

The source game is copied into a unique temporary directory **before the core receives its path**. Writes to relative game saves/configuration therefore affect the disposable copy. Original saves are never passed to the core as writable paths. Copying a large game takes disk space and time; symbolic links and special files are rejected. This is isolation of ordinary core storage, not an operating-system sandbox for untrusted executable cores. Configuration files that deliberately point outside the game must be adjusted to a temporary location.

Core runs launch a child process with temporary HOME/AppData/XDG/cache/TEMP locations before native workers start. A declared string `save_root` is always bound to a dedicated temporary save directory, overriding any supplied value. This also isolates adapters whose default saves normally live outside the game. Output is rejected inside the source game, and output/baseline directories must differ.

The owner thread and default Rust native-worker stack budget are 8 MiB. A larger explicit `RUST_MIN_STACK` is preserved for cores with large debug-build boot temporaries. A core that explicitly chooses its own stack remains responsible for that choice.

Native size, position, mode, visibility and minimize requests operate on a coherent virtual display state, never an OS window. State queries return that virtual state. Interactive modal requests fail explicitly; the host never invents a dialog answer.

`--configuration config.json` reads the existing Manager `ConfigEntry` array, for example `[{"id":"profile_path","value":{"String":"key.toml"}}]`; use the selected core's actual declared field IDs.

The host waits up to `--startup-timeout-ms` (default 30000, after native open) for a completed CPU frame. Rendering and game scripts remain enabled. No Slint window, display surface or event loop is created for core runs. Cores own their own graphics/audio devices; a GPU or audio device required by a core remains required. The runner does not silently disable audio, movies or script work.

### Clock and frame meaning

C ABI v3 gives the core ownership of its game clock and producer thread. It does not expose deterministic single-step/emulated time. The runner samples completed frames on the existing protocol's 16,666,667 ns host timeline, starting after the first completed frame. `--frames N` means N host presentation samples, **not** an instruction to execute exactly N native VM ticks. `acquired_frames` counts distinct completed leases actually acquired. A static title frame can validly appear at several sample points. Exact cross-run matching depends on the core/game's native timing and deterministic state; PNG tolerance is available when needed.

`--duration-ms` is a minimum timeline length, combined with `--frames`. `--capture-frame` is one-based; `--capture-ms` rounds upward to the next host sample. The last sample is always captured. Keep the requested timeline long enough for replay assertions and loading.

## Replay

`--script replay.json` loads a JSON array of existing protocol `InputMessage` records. Sequence numbers strictly increase, ticks never decrease and session stays constant. Pointer coordinates use the protocol's normalized 0..65535 range, mapped to the displayed frame's raster pixels as required by the native ABI. The core maps raster to logical coordinates; the runner must not apply that scaling twice. A click is move + press + release:

```json
[
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":1,"tick":0,"event":{"type":"advance_ticks","count":30}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":2,"tick":30,"event":{"type":"pointer_move","x":32768,"y":32768}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":3,"tick":30,"event":{"type":"pointer_button","button":"primary","state":"pressed"}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":4,"tick":31,"event":{"type":"pointer_button","button":"primary","state":"released"}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":5,"tick":40,"event":{"type":"keyboard","physical_key":"Enter","logical_key":"Enter","state":"pressed","repeat":false}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":6,"tick":41,"event":{"type":"keyboard","physical_key":"Enter","logical_key":"Enter","state":"released","repeat":false}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":7,"tick":60,"event":{"type":"await","observation":{"kind":"exists","key":"frame"},"timeout_ticks":60}},
  {"schema":"roukaku.user_input_sequence.v1","session":"game","sequence":8,"tick":60,"event":{"type":"checkpoint","id":"after-confirm"}}
]
```

Supported events: `resume`, `focus`, `keyboard`, `ime_commit`, `pointer_move`, `pointer_button`, `wheel`, `advance_ticks`, `await`, `checkpoint`. Unsupported protocol events fail explicitly. Keyboard names are the Manager's physical names (`KeyA`, `Enter`, `ControlLeft`, `ArrowDown`, `F1`, etc.). Each press/release is explicit. `advance_ticks` delays subsequent messages while preserving scheduled captures. `await` supports `exists` with key `frame`, or `equals` with key `frame.sha256` and `value_hash: "sha256:<64 hex>"`. An unmet assertion fails after `timeout_ticks`; `continue_at_match` must remain false because native producer clocks cannot be rewound.

Checkpoints export `<id>.png` and compare that name in `--baseline`. IDs must be unique flat filenames; `frame-` is reserved for scheduled captures. `--baseline-hash sha256:<64 hex>` asserts the final canonical pixel hash.

## Output and exit codes

Both core and UI captures use `<id>.png` plus `<id>.json`. The sidecar contains dimensions, sample number, canonical pixel hash, PNG-file hash and optional comparison metrics. Core success adds `run.json`; a core command failure writes `failure.json` with its diagnostic and exit code. A failed comparison retains the actual PNG and comparison sidecar.

Canonical pixels are top-down, tightly packed RGBA8 sRGB, alpha 255. BGRA, padded stride, bottom-up rows and linear-sRGB frames are normalized before SHA-256. `pixel_hash` is SHA-256 of those bytes, prefixed `sha256:`; `png_hash` hashes the encoded file. PNG encoder variation therefore does not change the pixel hash. With zero tolerance, baseline comparison is exact. With tolerance, a pixel differs only if any RGBA channel delta exceeds `--max-channel-delta`; the differing fraction must not exceed `--changed-pixel-ratio`. Dimensions must always match.

Exit codes: 0 success, 1 runtime/input/file/ABI/no-frame failure, 2 command-line syntax error (standalone Clap entrypoint), 3 baseline or replay assertion failure. Manager retains its existing syntax-error behavior.

## Manager UI without a display

```powershell
shinki-emu-headless ui --page library --language zh-CN --theme dark --width 1600 --height 1000 --output artifacts/ui/dark
shinki-emu-headless ui --page cores --language en --theme light --output artifacts/ui/light
shinki-emu-headless ui --page patches --language ja --theme dark --output artifacts/ui/dark
shinki-emu-headless ui --page settings --settings-category library-roots --language en --output artifacts/ui/dark
```

Pages are `library`, `cores`, `patches`, `settings`. Languages are `zh-CN` (also `zh-cn`), `en`, `ja`; themes are `dark` and `light`. Width/height are DPR-1 pixels in 360..8192. The actual `ManagerWindow` and `SlintManagerAdapter` render through `MinimalSoftwareWindow`, reusing the existing OHOS software-renderer path. This does not draw an imitation of the UI and creates no native window. The mode never starts Manager's database, library scanner, account, store or network services.

Default data is self-authored synthetic text with no covers, game executables or commercial screenshots. Optional `--demo-data demo.json` accepts:

```json
{"games":[{"title":"My open demo","description":"Self-authored example"}],"roots":["C:/ShinkiEMU/Demo"]}
```

This file supplies display-only strings, not filesystem roots to scan. No images are loaded from it. UI outputs are `<page>-<language>.png`, matching hash sidecars, and a `.source.json` describing the renderer and synthetic source. Use separate output directories for themes. Settings categories are `appearance`, `library-roots`, `network`, `translation`, `input`, `about`.
