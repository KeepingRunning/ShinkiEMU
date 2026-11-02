<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme-banner-dark.png">
  <img src="docs/assets/readme-banner-light.png" alt="ShinkiEMU: Game library manager and native cores for visual novel engines. Banner with the Project-Shinkirou pavilion mark and a pavilion on the horizon reflected in water.">
</picture>

# ShinkiEMU

A game library manager with native cores that run visual novels made with legacy engines.

[![CI](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml/badge.svg)](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Project-Shinkirou/ShinkiEMU?include_prereleases&sort=date)](https://github.com/Project-Shinkirou/ShinkiEMU/releases)

English · [简体中文](README.zh-CN.md) · [日本語](README.ja.md)

ShinkiEMU is pre-release software. Builds are published as beta releases. A successful build does not mean that a given game is verified playable; check the [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for per-game results.

**Portal:** [Download](https://project-shinkirou.github.io/en/shinkiemu/#download) · [Compatibility](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) · [Cores](https://project-shinkirou.github.io/en/shinkiemu/cores/) · [Patches](https://project-shinkirou.github.io/en/shinkiemu/patches/) · [FAQ](https://project-shinkirou.github.io/en/shinkiemu/faq/) · [Project status](https://project-shinkirou.github.io/en/status/)

## Features

- **Manager.** One desktop and mobile app for your game library, cores, metadata (Bangumi, VNDB) and store accounts (Steam, DLsite, DMM).
- **Native cores.** Each core keeps its engine's own VM, rendering, audio, menus and saves. The Manager hosts cores through one C ABI ([Core ABI](docs/en/core-abi.md)).
- **Separately released cores.** Cores are released and updated independently of the Manager, and installed from the in-app catalog.
- **Mobile embedding.** Android and iOS Managers link the supported cores statically; desktop Managers load downloaded core libraries.
- **Upstream engines stay upstream.** External cores are thin adapters on organization forks of existing open-source engine reimplementations. Their saves, keys and settings keep upstream locations and formats.
- **Your game directory stays clean.** Files ShinkiEMU itself writes in a game directory go under `<game>/.shinki/`.

## Supported engines

| Core | Engine | Desktop (dynamic core) | Embedded in Manager |
|---|---|---|---|
| [`rfvp`](docs/en/cores/rfvp.md) | FVP | Windows x64, Linux x64, macOS x64/arm64 | Android, iOS |
| [`siglus_rs`](docs/en/cores/siglus-rs.md) | SiglusEngine | Windows x64, Linux x64, macOS x64/arm64 | Android, iOS |
| [`sena-rs`](docs/en/cores/sena-rs.md) | SoftPAL | Windows x64, Linux x64, macOS x64/arm64 | iOS |
| [`ethornell-rs`](docs/en/cores/ethornell-rs.md) | BGI / Ethornell | Windows x64, Linux x64, macOS x64/arm64 | iOS |
| [`ReMinori`](docs/en/cores/reminori.md) | Musica | Windows x64, Linux x64, macOS x64/arm64 | iOS |
| [`shinki.emu.musica`](docs/en/cores/shinki-emu-musica.md) | Musica | Windows x64, Linux x64, macOS x64/arm64 | Android, iOS, experimental OHOS |
| [`art3m1s-core`](docs/en/cores/art3m1s-core.md) | Artemis | Windows x64, Linux x64, macOS x64/arm64 | Android |
| [`renpy`](docs/en/cores/renpy.md) | Ren'Py | Windows x64, Linux x64, macOS x64/arm64 | Android |
| [`shinki.emu.cmvs`](docs/en/cores/shinki-emu-cmvs.md) | CMVS | Windows x64, Linux x64, macOS x64/arm64 | — |
| [`kirikiroid2-web`](docs/en/cores/kirikiroid2-web.md) | KiriKiri | Windows x64, Linux x64, macOS x64/arm64 (also packaged as a standalone Android and iOS core) | — |
| [`impacto`](docs/en/cores/impacto.md) | MAGES. | Windows x64, Linux x64, macOS x64/arm64 | Android |

[`build/cores.toml`](build/cores.toml) and [`build/targets.toml`](build/targets.toml) define this matrix; run `cargo xtask matrix` for every combination with its blocked reason. The [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) lists what is actually published. The experimental OpenHarmony Manager embeds Musica only; its HAP uses the SDK test PKI, not retail provisioning. Other OHOS cores remain blocked.

## Install

1. Download the Manager bundle for your platform from the [portal](https://project-shinkirou.github.io/en/shinkiemu/#download) or [Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases).
2. Extract it and start `shinki-emu-manager` (`shinki-emu-manager.exe` on Windows).
3. On Linux, install the runtime libraries listed in [Build and release](docs/en/build-and-release.md#linux-desktop).

Published Android installers use the release signing key; debug-signed CI APKs are test artifacts. [Manager beta.6 release notes](docs/en/releases/manager-0.1.0-beta.6.md) describe its six embedded Android cores, retained iOS scope and native-capability requirements. Typed upstream-identity core packages require beta.6 or later; older Managers keep using the legacy core v1 and compatibility v4 catalogs.

## Quick start

1. In Settings, add the directories that contain your games.
2. Install or update the cores your games need from the core catalog.
3. Select a game in the library and launch it.

The [user guide](docs/en/user-guide.md) covers configuration, saves, metadata and patches.

## Building from source

Clone with submodules and use the pinned Rust 1.99.1 toolchain. All build, packaging and release steps are `cargo xtask` commands run from the repository root:

```sh
git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU
cd ShinkiEMU
cargo xtask doctor --target x86_64-pc-windows-msvc          # report missing prerequisites
cargo xtask deps --target x86_64-pc-windows-msvc --all-cores
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --core rfvp --release
cargo xtask test --core rfvp
```

`deps` fetches each external core's fork at the exact revision pinned in `build/cores.toml`, along with its locked native dependencies. Platform prerequisites (MSVC, Linux packages, Android NDK, the Ren'Py SDK), packaging and releases are in [Build and release](docs/en/build-and-release.md).

## Documentation

| | 简体中文 | English | 日本語 |
|---|---|---|---|
| Overview | [index](docs/zh-cn/index.md) | [index](docs/en/index.md) | [index](docs/ja/index.md) |
| User guide | [user-guide](docs/zh-cn/user-guide.md) | [user-guide](docs/en/user-guide.md) | [user-guide](docs/ja/user-guide.md) |
| Core ABI | [core-abi](docs/zh-cn/core-abi.md) | [core-abi](docs/en/core-abi.md) | [core-abi](docs/ja/core-abi.md) |
| Build and release | [build-and-release](docs/zh-cn/build-and-release.md) | [build-and-release](docs/en/build-and-release.md) | [build-and-release](docs/ja/build-and-release.md) |
| Metadata catalog | [metadata](docs/zh-cn/metadata.md) | [metadata](docs/en/metadata.md) | [metadata](docs/ja/metadata.md) |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. To write a new core or bring an existing engine in as a core, follow the agent skills in [`.agents/skills/`](.agents/skills/). Never commit commercial game data; tests use synthetic fixtures.

## Reporting issues

Common questions are answered in the [FAQ](https://project-shinkirou.github.io/en/shinkiemu/faq/). For anything else, use the issue forms:

- [Bug report](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=bug.yml)
- [Game compatibility report](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml)
- [Patch submission](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml)

Problems in an external engine itself may also belong upstream; each core page links its upstream project.

## Acknowledgements

The external cores are thin adapters on these open-source engine projects. Thank you to their authors and contributors. The Musica (`shinki.emu.musica`) and CMVS (`shinki.emu.cmvs`) cores are ShinkiEMU's own implementations and have no upstream project.

| Core | Engine family | Upstream project | Author or organization | License |
|---|---|---|---|---|
| `rfvp` | FVP | [xmoezzz/rfvp](https://github.com/xmoezzz/rfvp) | xmoe ([@xmoezzz](https://github.com/xmoezzz)) | [MPL-2.0](https://github.com/xmoezzz/rfvp/blob/99435f22e94eaec27c0c1b9f22cc2a6b1910d7d1/LICENSE) |
| `siglus_rs` | SiglusEngine | [xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs) | xmoe ([@xmoezzz](https://github.com/xmoezzz)) | [MPL-2.0](https://github.com/xmoezzz/siglus_rs/blob/5f358ef1360b0439e92aed2b749e8f25d8721954/LICENSE-MPL-2.0) |
| `sena-rs` | SoftPAL | [xmoezzz/sena-rs](https://github.com/xmoezzz/sena-rs) | xmoe ([@xmoezzz](https://github.com/xmoezzz)) | [MPL-2.0](https://github.com/xmoezzz/sena-rs/blob/7f633dbb89408297827d6c1857eb8e97dc363dff/LICENSE-MPL-2.0) |
| `ethornell-rs` | BGI / Ethornell | [xmoezzz/ethornell-rs](https://github.com/xmoezzz/ethornell-rs) | xmoe ([@xmoezzz](https://github.com/xmoezzz)) | MPL-2.0, declared in the [workspace manifest](https://github.com/xmoezzz/ethornell-rs/blob/b5de417e4cff681bd2afb929e22e903d64602456/Cargo.toml) |
| `ReMinori` | Musica | [luoyily/ReMinori](https://github.com/luoyily/ReMinori) | [@luoyily](https://github.com/luoyily) | [AGPL-3.0](https://github.com/luoyily/ReMinori/blob/79b00990324404bfbd496e7fa76c690cf040f744/LICENSE) |
| `art3m1s-core` | Artemis | [Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core) | Alphaly ([@Alphaly2K](https://github.com/Alphaly2K)) | [MPL-2.0](https://github.com/Alphaly2K/art3m1s-core/blob/ad90a62cb192b119b7cf8589aa7f87a94bd747c8/LICENSE); `crates/pf8` is [MIT](https://github.com/Alphaly2K/art3m1s-core/blob/ad90a62cb192b119b7cf8589aa7f87a94bd747c8/crates/pf8/LICENSE) |
| `renpy` | Ren'Py | [renpy/renpy](https://github.com/renpy/renpy) | The Ren'Py Visual Novel Engine Project ([@renpy](https://github.com/renpy)); source copyright Tom Rothamel | [MIT, with LGPL portions](https://github.com/renpy/renpy/blob/39895c1e017f0b36ffea2447d97eccd69d76ee1c/sphinx/source/license.rst) |
| `kirikiroid2-web` | KiriKiri | [fenghengzhi/kirikiroid2-web](https://github.com/fenghengzhi/kirikiroid2-web), a fork of [2468785842/krkr2](https://github.com/2468785842/krkr2) | [@fenghengzhi](https://github.com/fenghengzhi); KiriKiri2 copyright W.Dee and contributors | [KiriKiri2 license and bundled component licenses](https://github.com/fenghengzhi/kirikiroid2-web/blob/2028cbccaa91498bd36f270d349ae711d0e8fed3/LICENSE) |
| `impacto` | MAGES. | [CommitteeOfZero/impacto](https://github.com/CommitteeOfZero/impacto) | Committee of Zero and contributors ([@CommitteeOfZero](https://github.com/CommitteeOfZero)) | [ISC](https://github.com/CommitteeOfZero/impacto/blob/3706ddd89d4ad4aed0e32e127355c4251f22e574/LICENSE) |

License links point to the upstream revision pinned in [`build/cores.toml`](build/cores.toml). [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) has the full notices for the cores and their dependencies.

## License

Project-owned code is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. External cores, dependencies and assets keep their own licenses: see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and [ORIGINS.md](ORIGINS.md). Each core release ships a sources archive with the corresponding source.

[RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine) is the separate creation engine that ShinkiEMU's shared libraries come from.
