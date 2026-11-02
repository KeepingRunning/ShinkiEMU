<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme-banner-dark.png">
  <img src="docs/assets/readme-banner-light.png" alt="ShinkiEMU: Game library manager and native cores for visual novel engines. Banner with the Project-Shinkirou pavilion mark and a pavilion on the horizon reflected in water.">
</picture>

# ShinkiEMU

ShinkiEMU plays visual novels made with older engines on today's computers and phones. One Manager app keeps your game library, installs a native core for each engine, fetches metadata from Bangumi and VNDB, and downloads games you own from Steam and DLsite.

[![CI](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml/badge.svg)](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Project-Shinkirou/ShinkiEMU?include_prereleases&sort=date)](https://github.com/Project-Shinkirou/ShinkiEMU/releases)

English · [简体中文](README.zh-CN.md) · [日本語](README.ja.md)

ShinkiEMU is pre-release software. Builds are published as beta releases. A successful build does not mean that a given game is verified playable; check the [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for per-game results.

**Portal:** [Download](https://project-shinkirou.github.io/en/shinkiemu/#download) · [Compatibility](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) · [Cores](https://project-shinkirou.github.io/en/shinkiemu/cores/) · [Patches](https://project-shinkirou.github.io/en/shinkiemu/patches/) · [FAQ](https://project-shinkirou.github.io/en/shinkiemu/faq/) · [Project status](https://project-shinkirou.github.io/en/status/)

## Compatibility

ShinkiEMU supports ten engine families through eleven cores. Each core keeps its engine's own VM, rendering, audio, menus and saves. Nine cores are adapters on existing open-source engine reimplementations (see [Acknowledgements](#acknowledgements)); Musica and CMVS are ShinkiEMU's own implementations. [`build/cores.toml`](build/cores.toml) defines this list.

| Core | Engine family | What the core reads |
|---|---|---|
| [`rfvp`](docs/en/cores/rfvp.md) | FVP | HCB bytecode |
| [`siglus_rs`](docs/en/cores/siglus-rs.md) | SiglusEngine | `Scene.pck` scene packages |
| [`sena-rs`](docs/en/cores/sena-rs.md) | SoftPAL | `SCRIPT.SRC` bytecode with its `.DAT` tables; scope is the native and Steam releases of Koikake |
| [`ethornell-rs`](docs/en/cores/ethornell-rs.md) | BGI / Ethornell | BCS scenarios, BP system programs and DSC-compressed archive entries |
| [`ReMinori`](docs/en/cores/reminori.md) | Musica | PAZ archives; built around eden* |
| [`shinki.emu.musica`](docs/en/cores/shinki-emu-musica.md) | Musica | PAZ archives and SC scripts; original eden/Perseus `.sav` saves |
| [`art3m1s-core`](docs/en/cores/art3m1s-core.md) | Artemis | `system.ini`, PFS (PF6/PF8) archives and numbered `.pfs.NNN` patch volumes |
| [`renpy`](docs/en/cores/renpy.md) | Ren'Py | Runs the official Ren'Py 8.5.3 runtime |
| [`shinki.emu.cmvs`](docs/en/cores/shinki-emu-cmvs.md) | CMVS | CPZ5 archives and PS2A scripts |
| [`kirikiroid2-web`](docs/en/cores/kirikiroid2-web.md) | KiriKiri | XP3 archives and KAG/TJS scripts |
| [`impacto`](docs/en/cores/impacto.md) | MAGES. | MAGES. SC3 scripts with per-game configuration |

Recognizing an engine is not a playability result. Results for specific games, platforms and releases are on the portal's [compatibility page](https://project-shinkirou.github.io/en/shinkiemu/compatibility/); a game without a report is unreported. To add one, use the [game compatibility report](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml) form.

## Extensibility

- **Cores are plugins.** Cores are released and updated independently of the Manager. Desktop Managers install and update them from the in-app core catalog; Android and iOS Managers link their cores statically.
- **Third-party cores follow upstream.** External cores are thin adapters on organization forks. Their saves, keys and settings keep upstream locations and formats. Current packages are named after the upstream release they are built from, such as rfvp `0.7.0`, impacto `0.10.8.930` and Ren'Py `8.5.3.26051504`; Manager beta.6 or later reads them, while older Managers keep the legacy catalog.
- **Patches.** The portal's [patch library](https://project-shinkirou.github.io/en/shinkiemu/patches/) publishes reviewed Luau recipes that run only when you select them. By default a patch writes a new directory and leaves the original game unchanged. In-place mode backs up changed files under `<game>/.shinki/backup/` and can restore the latest patch. Hosted patch files are downloaded over HTTPS and checked against their size and SHA-256. Submit a new patch with the [patch submission](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml) form.
- **Store downloads.** Sign in to Steam or DLsite inside the Manager, pick a PC game your account owns, and download it into your library. Steam downloads can also check for updates and verify installed files. DMM GAMES/FANZA supports sign-in only; its downloads are not available yet. Logins are kept only in the platform's secret store. DRM and license checks remain the store's responsibility.

The [user guide](docs/en/user-guide.md) describes each of these in detail.

## Portability

| Core | Windows x64 | Linux x64 | macOS x64/arm64 | Android | iOS | OpenHarmony |
|---|---|---|---|---|---|---|
| `rfvp` | ✓ | ✓ | ✓ | Built in | Built in | — |
| `siglus_rs` | ✓ | ✓ | ✓ | Built in | Built in | — |
| `sena-rs` | ✓ | ✓ | ✓ | — | Built in | — |
| `ethornell-rs` | ✓ | ✓ | ✓ | — | Built in | — |
| `ReMinori` | ✓ | ✓ | ✓ | — | Built in | — |
| `shinki.emu.musica` | ✓ | ✓ | ✓ | Built in | Built in | Built in |
| `art3m1s-core` | ✓ | ✓ | ✓ | Built in | — | — |
| `renpy` | ✓ | ✓ | ✓ | Built in | — | — |
| `shinki.emu.cmvs` | ✓ | ✓ | ✓ | — | — | — |
| `kirikiroid2-web` | ✓ | ✓ | ✓ | Built in | — | — |
| `impacto` | ✓ | ✓ | ✓ | Built in | — | — |

✓ means the core is published in the desktop core catalog. "Built in" is the set embedded in [Manager 0.1.0-beta.7](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.7): seven cores on Android, six on iOS and Musica on OpenHarmony. Android and iOS Managers use their built-in cores; desktop Managers load cores downloaded from the catalog.

- **Windows, Linux and macOS:** ZIP bundles for x64, plus Apple Silicon on macOS.
- **Android:** one universal APK signed with the release key. Debug-signed CI APKs are test builds.
- **iOS:** device and simulator packages, published as test builds.
- **OpenHarmony:** an experimental HAP signed with the OpenHarmony SDK test PKI. Launch on a device and retail installation have not been verified.

Save behavior is core-specific. External cores keep their upstream save locations and formats. Musica stores original-format eden/Perseus `.sav` files under `<game>/.shinki/musica/`. Files that ShinkiEMU itself writes in a game directory go under `<game>/.shinki/`. Close the game before you back up saves or exchange supported original-format save files.

## Download and quick start

1. Download the Manager bundle for your platform from the [portal](https://project-shinkirou.github.io/en/shinkiemu/#download) or [Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases).
2. Extract it and start `shinki-emu-manager` (`shinki-emu-manager.exe` on Windows). On Linux, install the runtime libraries listed in [Build and release](docs/en/build-and-release.md#linux-desktop) first.
3. In Settings, add the directories that contain your games.
4. Install or update the cores your games need from the core catalog.
5. Select a game in the library and launch it.

The Manager is available in Simplified Chinese, English and Japanese. The [user guide](docs/en/user-guide.md) covers configuration, saves, metadata, store downloads and patches. Release notes: [Manager beta.7](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.7) · [beta.6](docs/en/releases/manager-0.1.0-beta.6.md).

## Reporting issues

Common questions are answered in the [FAQ](https://project-shinkirou.github.io/en/shinkiemu/faq/). For anything else, use the issue forms:

- [Bug report](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=bug.yml)
- [Game compatibility report](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml)
- [Patch submission](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml)

Problems in an external engine itself may also belong upstream; each core page links its upstream project.

## Development

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Build, packaging and release steps, including platform prerequisites and the pinned toolchain, are in [Build and release](docs/en/build-and-release.md). To write a new core or bring an existing engine in as a core, follow the agent skills in [`.agents/skills/`](.agents/skills/). Never commit commercial game data; tests use synthetic fixtures.

| | 简体中文 | English | 日本語 |
|---|---|---|---|
| Overview | [index](docs/zh-cn/index.md) | [index](docs/en/index.md) | [index](docs/ja/index.md) |
| User guide | [user-guide](docs/zh-cn/user-guide.md) | [user-guide](docs/en/user-guide.md) | [user-guide](docs/ja/user-guide.md) |
| Core ABI | [core-abi](docs/zh-cn/core-abi.md) | [core-abi](docs/en/core-abi.md) | [core-abi](docs/ja/core-abi.md) |
| Build and release | [build-and-release](docs/zh-cn/build-and-release.md) | [build-and-release](docs/en/build-and-release.md) | [build-and-release](docs/ja/build-and-release.md) |
| Metadata catalog | [metadata](docs/zh-cn/metadata.md) | [metadata](docs/en/metadata.md) | [metadata](docs/ja/metadata.md) |
| Headless runner | [headless](docs/zh-cn/headless.md) | [headless](docs/en/headless.md) | [headless](docs/ja/headless.md) |

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
