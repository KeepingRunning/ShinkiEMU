# Origins

ShinkiEMU starts a new Git history. Its code and data come from repositories by the same authors; the original histories stay there.

| Source | Revision | Used for |
|---|---|---|
| https://github.com/2018wzh/AstraEngine (`master`) | `1576884396ce7803c267dbc5119135cc7eb6a715` | `Emulator/` lifted to the repository root (`Source/`, `Assets/Effects`, `Platforms/`, `Schemas/`, `ThirdParty/mlua`), `Tools/AstraEMU` research scripts → `Tools/Research`, FFmpeg vcpkg manifest → `build/vcpkg/ffmpeg` |
| same repository, branch `codex/cmvs-independent-host-20261011` | `8b6a31f6123ec14cfb9236ee3b7c9095b7c8ce66` plus the uncommitted `scene.rs`/`session.rs` changes of that worktree on 2026-10-23 | CMVS core (`Source/Families/shinki-emu-cmvs`) |
| same repository, branch `emu/reminori-impacto-20261023` | `54306c903954dadca7d3d65de486a1736bff40e3` | ReMinori and impacto integration, Metadata core registration |
| same repository, branch `emu/renpy-embed` | `39c81a31be241965866dff146f56616f1e873846` | Ren'Py integration, pickle/RPA support in `shinki-emu-patch` |
| https://github.com/2018wzh/AstraEMU-metadata (`cores/renpy-20261023`) | `cdf46716b30d35c0ea1ec46b310dec48748e5c5d` | `Metadata/` (now a plain directory) |
| AstraEngine `Examples/NativeVN/Assets/Fonts`, `Engine/Fixtures/PublicDomainFonts` | as above | `Assets/Fonts` (Noto Sans JP/SC, SIL OFL 1.1) |
| AstraEngine `Engine/Fixtures/PublicDomainMedia/flower-roar.mp4` | as above | `Fixtures/Media` (derived from MDN interactive-examples media, CC0-1.0) |
| https://github.com/Project-Shinkirou/Project-Shinkirou.github.io (`main`) | `59853af07f2a6ed32bc21b4ab6d014287f8f3814` | `docs/assets/readme-banner-{dark,light}.png`, rendered from the portal's `MirageScene.astro` night scene, `theme.css` night and dawn tokens, and `LogoMark.astro` mark |

External cores live in organization forks that keep their upstream history and attribution; ShinkiEMU pins them as submodules on their `shinki` branches (`cpal` and `kira` on version maintenance branches).

All own identifiers were renamed (`astra-emu-*` → `shinki-emu-*`, `astra_emu_query` → `shinki_emu_query`, `astra.emu.*` → `shinki.emu.*`, shared engine crates `astra-*` → `roukaku-*` from [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine)). Data directories, packages, caches and saves written by AstraEMU are not read by ShinkiEMU; original game saves are untouched.
