# ShinkiEMU

游戏库管理器加一组原生核心，用来运行旧视觉小说引擎制作的游戏。

[![CI](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml/badge.svg)](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Project-Shinkirou/ShinkiEMU?include_prereleases&sort=date)](https://github.com/Project-Shinkirou/ShinkiEMU/releases)

[English](README.md) · 简体中文 · [日本語](README.ja.md)

ShinkiEMU 仍处于预发布阶段，构建以 beta 版发布。构建成功不等于某个游戏已验证可玩，逐个游戏的结论见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。

**门户：**[下载](https://project-shinkirou.github.io/zh-cn/shinkiemu/#download) · [兼容性](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/) · [核心](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/) · [补丁](https://project-shinkirou.github.io/zh-cn/shinkiemu/patches/) · [常见问题](https://project-shinkirou.github.io/zh-cn/shinkiemu/faq/) · [项目状态](https://project-shinkirou.github.io/zh-cn/status/)

## 功能

- **Manager**：一个桌面与移动端应用，管理游戏库、核心、元数据（Bangumi、VNDB）和商店账号（Steam、DLsite、DMM）。
- **原生核心**：每个核心保留引擎自己的 VM、渲染、音频、菜单和存档。Manager 通过统一的 C ABI 加载核心，见[核心 ABI](docs/zh-cn/core-abi.md)。
- **核心单独发布**：核心与 Manager 各自发布和更新，在应用内的核心目录里安装。
- **移动端内嵌**：Android 与 iOS 版 Manager 静态链接所支持的核心；桌面版加载下载的核心动态库。
- **外部引擎按上游行为运行**：外部核心是组织 fork 上的薄适配层，底下是现有的开源引擎复刻实现。存档、密钥和设置沿用上游的位置与格式。
- **不弄乱游戏目录**：ShinkiEMU 自己在游戏目录里写的文件都放在 `<game>/.shinki/` 下。

## 支持的引擎

| 核心 | 引擎 | 桌面（动态核心） | 内嵌于 Manager |
|---|---|---|---|
| [`rfvp`](docs/zh-cn/cores/rfvp.md) | FVP | Windows x64、Linux x64、macOS x64/arm64 | Android、iOS |
| [`siglus_rs`](docs/zh-cn/cores/siglus-rs.md) | SiglusEngine | Windows x64、Linux x64、macOS x64/arm64 | Android、iOS |
| [`sena-rs`](docs/zh-cn/cores/sena-rs.md) | SoftPAL | Windows x64、Linux x64、macOS x64/arm64 | iOS |
| [`ethornell-rs`](docs/zh-cn/cores/ethornell-rs.md) | BGI / Ethornell | Windows x64、Linux x64、macOS x64/arm64 | iOS |
| [`ReMinori`](docs/zh-cn/cores/reminori.md) | Musica | Windows x64、Linux x64、macOS x64/arm64 | iOS |
| [`shinki.emu.musica`](docs/zh-cn/cores/shinki-emu-musica.md) | Musica | Windows x64、Linux x64、macOS x64/arm64 | Android、iOS、实验性 OHOS |
| [`art3m1s-core`](docs/zh-cn/cores/art3m1s-core.md) | Artemis | Windows x64、Linux x64、macOS x64/arm64 | Android |
| [`renpy`](docs/zh-cn/cores/renpy.md) | Ren'Py | Windows x64、Linux x64、macOS x64/arm64 | Android |
| [`shinki.emu.cmvs`](docs/zh-cn/cores/shinki-emu-cmvs.md) | CMVS | Windows x64、Linux x64、macOS x64/arm64 | — |
| [`kirikiroid2-web`](docs/zh-cn/cores/kirikiroid2-web.md) | KiriKiri | Windows x64、Linux x64、macOS x64/arm64（另有独立的 Android 和 iOS 核心包） | — |
| [`impacto`](docs/zh-cn/cores/impacto.md) | MAGES. | Windows x64、Linux x64、macOS x64/arm64 | Android |

这张表由 [`build/cores.toml`](build/cores.toml) 和 [`build/targets.toml`](build/targets.toml) 决定；`cargo xtask matrix` 会列出全部组合及 blocked 原因。实际发布了哪些产物，以[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)为准。实验性 OpenHarmony Manager 只内置 Musica；HAP 使用 SDK 测试 PKI 签名，未作零售设备配签。其他 OHOS 核心仍为 blocked。

## 安装

1. 从[门户](https://project-shinkirou.github.io/zh-cn/shinkiemu/#download)或 [Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases) 下载对应平台的 Manager 包。
2. 解压后运行 `shinki-emu-manager`（Windows 上是 `shinki-emu-manager.exe`）。
3. Linux 需要先装好[构建与发布](docs/zh-cn/build-and-release.md)里 Linux 桌面一节列出的运行库。

公开的 Android 安装包使用发布签名密钥，debug 签名的 CI APK 只作测试。[Manager beta.6 发布说明](docs/zh-cn/releases/manager-0.1.0-beta.6.md)列出其 Android 六核、保留的 iOS 范围与原生能力要求。带上游身份的新核心包需要 beta.6 或更高版本；旧 Manager 继续使用核心 v1 与兼容性 v4 目录。

## 快速上手

1. 在设置中添加存放游戏的目录。
2. 从核心目录安装或更新游戏需要的核心。
3. 在游戏库里选中游戏并启动。

配置、存档、元数据和补丁的用法见[用户指南](docs/zh-cn/user-guide.md)。

## 从源码构建

带子模块克隆，使用固定的 Rust 1.99.1 工具链。构建、打包和发布都是在仓库根目录运行的 `cargo xtask` 命令：

```sh
git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU
cd ShinkiEMU
cargo xtask doctor --target x86_64-pc-windows-msvc          # 列出缺少的前置条件
cargo xtask deps --target x86_64-pc-windows-msvc --all-cores
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --core rfvp --release
cargo xtask test --core rfvp
```

`deps` 按 `build/cores.toml` 固定的精确提交拉取各外部核心的 fork，并准备锁定版本的原生依赖。各平台前置条件（MSVC、Linux 软件包、Android NDK、Ren'Py SDK）以及打包、发布流程见[构建与发布](docs/zh-cn/build-and-release.md)。

## 文档

| | 简体中文 | English | 日本語 |
|---|---|---|---|
| 概览 | [index](docs/zh-cn/index.md) | [index](docs/en/index.md) | [index](docs/ja/index.md) |
| 用户指南 | [user-guide](docs/zh-cn/user-guide.md) | [user-guide](docs/en/user-guide.md) | [user-guide](docs/ja/user-guide.md) |
| 核心 ABI | [core-abi](docs/zh-cn/core-abi.md) | [core-abi](docs/en/core-abi.md) | [core-abi](docs/ja/core-abi.md) |
| 构建与发布 | [build-and-release](docs/zh-cn/build-and-release.md) | [build-and-release](docs/en/build-and-release.md) | [build-and-release](docs/ja/build-and-release.md) |
| 元数据目录 | [metadata](docs/zh-cn/metadata.md) | [metadata](docs/en/metadata.md) | [metadata](docs/ja/metadata.md) |

## 参与贡献

提交 PR 前先读 [CONTRIBUTING.md](CONTRIBUTING.md)。编写新核心，或把现有引擎接入为核心，按 [`.agents/skills/`](.agents/skills/) 里的 agent skill 操作。不要提交商业游戏数据，测试只用自造夹具。

## 问题反馈

常见问题见[常见问题](https://project-shinkirou.github.io/zh-cn/shinkiemu/faq/)页面。其他问题请使用 issue 表单：

- [Bug 报告](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=bug.yml)
- [游戏兼容性报告](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml)
- [补丁提交](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml)

外部引擎本身的问题也可以报给上游，每个核心页面都链接了它的上游项目。

## 许可

项目自有代码采用 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE) 双许可，任选其一。外部核心、依赖和素材保留各自的许可，见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) 和 [ORIGINS.md](ORIGINS.md)。每个核心发布都附带对应源码的 sources 压缩包。

ShinkiEMU 的共享底层库来自独立的创作引擎 [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine)。
