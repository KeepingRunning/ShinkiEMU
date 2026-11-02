<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme-banner-dark.png">
  <img src="docs/assets/readme-banner-light.png" alt="ShinkiEMU 横幅：产品名 ShinkiEMU 与标语 “Game library manager and native cores for visual novel engines”，配 Project-Shinkirou 楼阁标志，背景是地平线上的楼阁倒映在水面。">
</picture>

# ShinkiEMU

ShinkiEMU 让旧引擎制作的视觉小说在现在的电脑和手机上运行。一个 Manager 应用管理游戏库，为每种引擎安装原生核心，从 Bangumi 和 VNDB 获取元数据，还能从 Steam 和 DLsite 下载你已购买的游戏。

[![CI](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml/badge.svg)](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Project-Shinkirou/ShinkiEMU?include_prereleases&sort=date)](https://github.com/Project-Shinkirou/ShinkiEMU/releases)

[English](README.md) · 简体中文 · [日本語](README.ja.md)

ShinkiEMU 仍处于预发布阶段，构建以 beta 版发布。构建成功不等于某个游戏已验证可玩，逐个游戏的结论见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。

**门户：**[下载](https://project-shinkirou.github.io/zh-cn/shinkiemu/#download) · [兼容性](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/) · [核心](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/) · [补丁](https://project-shinkirou.github.io/zh-cn/shinkiemu/patches/) · [常见问题](https://project-shinkirou.github.io/zh-cn/shinkiemu/faq/) · [项目状态](https://project-shinkirou.github.io/zh-cn/status/)

## 兼容性

ShinkiEMU 用 11 个核心支持 10 个引擎家族。每个核心保留引擎自己的 VM、渲染、音频、菜单和存档。其中 9 个核心是现有开源引擎复刻实现上的适配层（见[致谢](#致谢)），Musica 和 CMVS 由 ShinkiEMU 自行实现。核心列表由 [`build/cores.toml`](build/cores.toml) 定义。

| 核心 | 引擎家族 | 核心读取的内容 |
|---|---|---|
| [`rfvp`](docs/zh-cn/cores/rfvp.md) | FVP | HCB 字节码 |
| [`siglus_rs`](docs/zh-cn/cores/siglus-rs.md) | SiglusEngine | `Scene.pck` 场景包 |
| [`sena-rs`](docs/zh-cn/cores/sena-rs.md) | SoftPAL | `SCRIPT.SRC` 字节码及配套 `.DAT` 表；适用范围为原版和 Steam 版 Koikake |
| [`ethornell-rs`](docs/zh-cn/cores/ethornell-rs.md) | BGI / Ethornell | BCS 剧本、BP 系统程序和 DSC 压缩的归档条目 |
| [`ReMinori`](docs/zh-cn/cores/reminori.md) | Musica | PAZ 归档；围绕 eden* 实现 |
| [`shinki.emu.musica`](docs/zh-cn/cores/shinki-emu-musica.md) | Musica | PAZ 归档和 SC 脚本；使用原版 eden/Perseus `.sav` 存档 |
| [`art3m1s-core`](docs/zh-cn/cores/art3m1s-core.md) | Artemis | `system.ini`、PFS（PF6/PF8）归档和编号补丁卷 `.pfs.NNN` |
| [`renpy`](docs/zh-cn/cores/renpy.md) | Ren'Py | 运行官方 Ren'Py 8.5.3 运行时 |
| [`shinki.emu.cmvs`](docs/zh-cn/cores/shinki-emu-cmvs.md) | CMVS | CPZ5 归档和 PS2A 脚本 |
| [`kirikiroid2-web`](docs/zh-cn/cores/kirikiroid2-web.md) | KiriKiri | XP3 归档和 KAG/TJS 脚本 |
| [`impacto`](docs/zh-cn/cores/impacto.md) | MAGES. | MAGES. SC3 脚本及逐游戏配置 |

能识别引擎不代表游戏可玩。具体游戏、平台和发行版本的结论见门户的[兼容性页面](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)，没有报告的游戏视为未报告。要提交结论，请使用[游戏兼容性报告](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml)表单。

## 扩展性

- **核心以插件形式提供**：核心与 Manager 各自发布和更新。桌面版 Manager 从应用内的核心目录安装和更新核心；Android 与 iOS 版 Manager 静态链接核心。
- **第三方核心跟随上游**：外部核心是组织 fork 上的薄适配层，存档、密钥和设置沿用上游的位置与格式。现在的核心包以构建所用的上游版本命名，例如 rfvp `0.7.0`、impacto `0.10.8.930`、Ren'Py `8.5.3.26051504`。这类核心包需要 Manager beta.6 或更高版本，旧 Manager 继续使用旧目录。
- **补丁**：门户[补丁库](https://project-shinkirou.github.io/zh-cn/shinkiemu/patches/)发布经过审核的 Luau 配方，只有在你选中时才会运行。补丁默认输出到新目录，原游戏保持不变；原位覆盖模式会把改动的文件备份到 `<game>/.shinki/backup/`，并可还原最近一次补丁。托管的补丁文件通过 HTTPS 下载，并校验大小和 SHA-256。新补丁可以用[补丁提交](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml)表单投稿。
- **商店下载**：在 Manager 里登录 Steam 或 DLsite，选择账号已拥有的 PC 游戏，下载到游戏库。Steam 下载还能检查更新、校验已安装的文件。DMM GAMES/FANZA 目前只能登录，下载功能暂未提供。登录信息只保存在系统的安全存储里。DRM 和授权检查仍由商店负责。

各项功能的详细说明见[用户指南](docs/zh-cn/user-guide.md)。

## 可移植性

| 核心 | Windows x64 | Linux x64 | macOS x64/arm64 | Android | iOS | OpenHarmony |
|---|---|---|---|---|---|---|
| `rfvp` | ✓ | ✓ | ✓ | 内置 | 内置 | — |
| `siglus_rs` | ✓ | ✓ | ✓ | 内置 | 内置 | — |
| `sena-rs` | ✓ | ✓ | ✓ | — | 内置 | — |
| `ethornell-rs` | ✓ | ✓ | ✓ | — | 内置 | — |
| `ReMinori` | ✓ | ✓ | ✓ | — | 内置 | — |
| `shinki.emu.musica` | ✓ | ✓ | ✓ | 内置 | 内置 | 内置 |
| `art3m1s-core` | ✓ | ✓ | ✓ | 内置 | — | — |
| `renpy` | ✓ | ✓ | ✓ | 内置 | — | — |
| `shinki.emu.cmvs` | ✓ | ✓ | ✓ | — | — | — |
| `kirikiroid2-web` | ✓ | ✓ | ✓ | 内置 | — | — |
| `impacto` | ✓ | ✓ | ✓ | 内置 | — | — |

✓ 表示该核心已在桌面版核心目录中发布。“内置”指 [Manager 0.1.0-beta.7](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.7) 内嵌的核心：Android 7 个，iOS 6 个，OpenHarmony 只有 Musica。Android 与 iOS 版 Manager 使用内置核心，桌面版加载从核心目录下载的核心。

- **Windows、Linux、macOS**：x64 的 ZIP 包，macOS 另有 Apple Silicon 版本。
- **Android**：一个通用 APK，使用发布签名密钥。debug 签名的 CI APK 只作测试。
- **iOS**：真机和模拟器包，作为测试构建发布。
- **OpenHarmony**：实验性 HAP，使用 OpenHarmony SDK 测试 PKI 签名，尚未验证真机启动和零售设备安装。

存档行为因核心而异。外部核心沿用上游的存档位置与格式；Musica 把原版格式的 eden/Perseus `.sav` 存放在 `<game>/.shinki/musica/` 下。ShinkiEMU 自己在游戏目录里写的文件都放在 `<game>/.shinki/` 下。备份存档或交换受支持的原版格式存档前，先关闭游戏。

## 下载与快速上手

1. 从[门户](https://project-shinkirou.github.io/zh-cn/shinkiemu/#download)或 [Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases) 下载对应平台的 Manager 包。
2. 解压后运行 `shinki-emu-manager`（Windows 上是 `shinki-emu-manager.exe`）。Linux 需要先装好[构建与发布](docs/zh-cn/build-and-release.md)里 Linux 桌面一节列出的运行库。
3. 在设置中添加存放游戏的目录。
4. 从核心目录安装或更新游戏需要的核心。
5. 在游戏库里选中游戏并启动。

Manager 界面提供简体中文、英文和日文。配置、存档、元数据、商店下载和补丁的用法见[用户指南](docs/zh-cn/user-guide.md)。发布说明：[Manager beta.7](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.7) · [beta.6](docs/zh-cn/releases/manager-0.1.0-beta.6.md)。

## 问题反馈

常见问题见[常见问题](https://project-shinkirou.github.io/zh-cn/shinkiemu/faq/)页面。其他问题请使用 issue 表单：

- [Bug 报告](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=bug.yml)
- [游戏兼容性报告](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml)
- [补丁提交](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml)

外部引擎本身的问题也可以报给上游，每个核心页面都链接了它的上游项目。

## 开发

提交 PR 前先读 [CONTRIBUTING.md](CONTRIBUTING.md)。构建、打包和发布步骤，包括各平台前置条件和固定的工具链，见[构建与发布](docs/zh-cn/build-and-release.md)。编写新核心，或把现有引擎接入为核心，按 [`.agents/skills/`](.agents/skills/) 里的 agent skill 操作。不要提交商业游戏数据，测试只用自造夹具。

| | 简体中文 | English | 日本語 |
|---|---|---|---|
| 概览 | [index](docs/zh-cn/index.md) | [index](docs/en/index.md) | [index](docs/ja/index.md) |
| 用户指南 | [user-guide](docs/zh-cn/user-guide.md) | [user-guide](docs/en/user-guide.md) | [user-guide](docs/ja/user-guide.md) |
| 核心 ABI | [core-abi](docs/zh-cn/core-abi.md) | [core-abi](docs/en/core-abi.md) | [core-abi](docs/ja/core-abi.md) |
| 构建与发布 | [build-and-release](docs/zh-cn/build-and-release.md) | [build-and-release](docs/en/build-and-release.md) | [build-and-release](docs/ja/build-and-release.md) |
| 元数据目录 | [metadata](docs/zh-cn/metadata.md) | [metadata](docs/en/metadata.md) | [metadata](docs/ja/metadata.md) |
| 无界面运行器 | [headless](docs/zh-cn/headless.md) | [headless](docs/en/headless.md) | [headless](docs/ja/headless.md) |

## 致谢

外部核心是以下开源引擎项目之上的薄适配层，感谢这些项目的作者和贡献者。Musica（`shinki.emu.musica`）与 CMVS（`shinki.emu.cmvs`）核心由 ShinkiEMU 自行实现，没有上游项目。

| 核心 | 引擎家族 | 上游项目 | 作者或组织 | 许可证 |
|---|---|---|---|---|
| `rfvp` | FVP | [xmoezzz/rfvp](https://github.com/xmoezzz/rfvp) | xmoe（[@xmoezzz](https://github.com/xmoezzz)） | [MPL-2.0](https://github.com/xmoezzz/rfvp/blob/99435f22e94eaec27c0c1b9f22cc2a6b1910d7d1/LICENSE) |
| `siglus_rs` | SiglusEngine | [xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs) | xmoe（[@xmoezzz](https://github.com/xmoezzz)） | [MPL-2.0](https://github.com/xmoezzz/siglus_rs/blob/5f358ef1360b0439e92aed2b749e8f25d8721954/LICENSE-MPL-2.0) |
| `sena-rs` | SoftPAL | [xmoezzz/sena-rs](https://github.com/xmoezzz/sena-rs) | xmoe（[@xmoezzz](https://github.com/xmoezzz)） | [MPL-2.0](https://github.com/xmoezzz/sena-rs/blob/7f633dbb89408297827d6c1857eb8e97dc363dff/LICENSE-MPL-2.0) |
| `ethornell-rs` | BGI / Ethornell | [xmoezzz/ethornell-rs](https://github.com/xmoezzz/ethornell-rs) | xmoe（[@xmoezzz](https://github.com/xmoezzz)） | MPL-2.0，见 [workspace manifest](https://github.com/xmoezzz/ethornell-rs/blob/b5de417e4cff681bd2afb929e22e903d64602456/Cargo.toml) 中的声明 |
| `ReMinori` | Musica | [luoyily/ReMinori](https://github.com/luoyily/ReMinori) | [@luoyily](https://github.com/luoyily) | [AGPL-3.0](https://github.com/luoyily/ReMinori/blob/79b00990324404bfbd496e7fa76c690cf040f744/LICENSE) |
| `art3m1s-core` | Artemis | [Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core) | Alphaly（[@Alphaly2K](https://github.com/Alphaly2K)） | [MPL-2.0](https://github.com/Alphaly2K/art3m1s-core/blob/ad90a62cb192b119b7cf8589aa7f87a94bd747c8/LICENSE)；`crates/pf8` 为 [MIT](https://github.com/Alphaly2K/art3m1s-core/blob/ad90a62cb192b119b7cf8589aa7f87a94bd747c8/crates/pf8/LICENSE) |
| `renpy` | Ren'Py | [renpy/renpy](https://github.com/renpy/renpy) | The Ren'Py Visual Novel Engine Project（[@renpy](https://github.com/renpy)）；源码版权人 Tom Rothamel | [MIT，部分为 LGPL](https://github.com/renpy/renpy/blob/39895c1e017f0b36ffea2447d97eccd69d76ee1c/sphinx/source/license.rst) |
| `kirikiroid2-web` | KiriKiri | [fenghengzhi/kirikiroid2-web](https://github.com/fenghengzhi/kirikiroid2-web)，fork 自 [2468785842/krkr2](https://github.com/2468785842/krkr2) | [@fenghengzhi](https://github.com/fenghengzhi)；KiriKiri2 版权人 W.Dee and contributors | [KiriKiri2 许可及所含组件的许可](https://github.com/fenghengzhi/kirikiroid2-web/blob/2028cbccaa91498bd36f270d349ae711d0e8fed3/LICENSE) |
| `impacto` | MAGES. | [CommitteeOfZero/impacto](https://github.com/CommitteeOfZero/impacto) | Committee of Zero and contributors（[@CommitteeOfZero](https://github.com/CommitteeOfZero)） | [ISC](https://github.com/CommitteeOfZero/impacto/blob/3706ddd89d4ad4aed0e32e127355c4251f22e574/LICENSE) |

许可证链接指向 [`build/cores.toml`](build/cores.toml) 固定的上游版本。核心及其依赖的完整声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

## 许可

项目自有代码采用 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE) 双许可，任选其一。外部核心、依赖和素材保留各自的许可，见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) 和 [ORIGINS.md](ORIGINS.md)。每个核心发布都附带对应源码的 sources 压缩包。

ShinkiEMU 的共享底层库来自独立的创作引擎 [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine)。
