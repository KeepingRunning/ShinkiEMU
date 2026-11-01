---
name: shinkiemu-integrate-existing-core
description: Use when porting an existing third-party engine implementation (a Rust crate or a C/C++ project) into ShinkiEMU as an external core, or when changing an existing fork adapter, its pin, upstream_identity or release_sequence, its Cargo.lock entries, its save/config behaviour, its static Manager embedding or its license and source bundle. Covers the Project-Shinkirou fork and shinki-<topic> adapter branches, the thin adapter crate or vendored C header, worker/frame-lease mapping, vcpkg dependencies and overlay ports, merging per-platform fork lines, ABI query probes and headless acceptance. Not for a new in-repo SDK core; use shinkiemu-new-core.
---

# 把第三方引擎移植进 ShinkiEMU

外部核心 = 原样保留的上游引擎 + 一个实现 C ABI v3 的薄 adapter。两者都在组织 fork 里。ShinkiEMU 用精确 commit 固定 fork，不复制 adapter 代码。

先确认该走移植路径：上游能跑，许可证允许再分发且我们能履行义务。否则用 `shinkiemu-new-core` 基于 SDK 自研（决策表在那里的 §0）。

契约文档：[Core ABI](../../../docs/en/core-abi.md)、[Build and release](../../../docs/en/build-and-release.md)、[ThirdParty/README](../../../ThirdParty/README.md)、[THIRD_PARTY_NOTICES](../../../THIRD_PARTY_NOTICES.md)、[ORIGINS](../../../ORIGINS.md)。运行模型（worker、帧、生命周期、boundary）见 `shinkiemu-new-core` §3–§4，构建/CI/发布流程见 `shinkiemu-maintenance`。

## 现有例子

| 核心 | 类型 | adapter | 移动端 Rust 路径 query |
|---|---|---|---|
| rfvp | Rust | `crates/shinki-emu-rfvp`；`#[cfg_attr(feature = "dynamic-plugin-export", no_mangle)]` 门控导出 | `shinki_emu_rfvp::shinki_emu_query`（Android、iOS） |
| siglus_rs | Rust | `shinki/shinki-emu-siglus_rs`，带 `[package.metadata.shinki]` 来源块 | `shinki_emu_siglus_rs::query`（Android、iOS） |
| art3m1s-core | Rust + Luau | `crates/shinki-emu-art3m1s-core`，静态 feature `static-core` | `shinki_emu_art3m1s_core::artemis_query`（Android） |
| sena-rs | Rust | `crates/shinki-emu-sena-rs`；另有 `pub const QUERY`；ready 握手在 `session.rs` | `shinki_emu_sena_rs::shinki_emu_query`（iOS） |
| ethornell-rs | Rust | `crates/shinki-emu-ethornell-rs`；另有 `pub const QUERY` | `shinki_emu_ethornell_rs::shinki_emu_query`（iOS） |
| ReMinori | Rust, AGPL | `crates/shinki`（`build.rs`、`MODIFICATIONS.md`） | `shinki_emu_ReMinori::shinki_emu_query`（iOS） |
| impacto | C++ | `shinki/` Cargo 包装 + `src/shinki/core.cpp`（worker `Run`、`GetFrame`）；`vendor/shinki-emu-abi/include/shinki_emu.h` | `shinki_emu_impacto::shinki_emu_query`（Android，feature `static`） |
| kirikiroid2-web | C++ | `shinki/`（`build.rs`、`native/NativeSession.cpp`、`NativeOwner.cpp`）；`shinki/native/include/shinki_emu.h` | `shinki_emu_kirikiroid2_web::query`（Android，feature `static`；在 beta7 候选 `candidate/manager-beta7-publishclose63` 上，尚未进 main） |
| renpy | C++ + Python | `shinki/core.cpp` + `shinki_renpy_bridge.py`，打包官方 Ren'Py SDK | `shinki_emu_renpy::query`（Android，feature `static`） |

fork 在 `ThirdParty/<dir>/`，由 `cargo xtask deps --core <id>` 检出到 pin。`kirikiri2/` 对应组织仓库 `kirikiroid2-web`。

## 1. fork 与分支

1. 在组织下 fork 为 `Project-Shinkirou/<上游仓库名>`。plugin_id 保留上游仓库名和大小写（`siglus_rs`、`ReMinori`、`sena-rs`）。保留上游历史、作者和默认分支。
2. 加 `upstream` remote。adapter 从一个明确的上游 commit 开始，最好是上游 tag。
3. 分支：`shinki` 是长期集成分支；平台或专题工作放在 `shinki-<topic>`（实际在用的有 `shinki-android-ports`、`shinki-android-isolation`、`shinki-apple-xplat`、`shinki-sakura-native`、`shinki-common63`）。`build/cores.toml` 的 `branch` 写 pin 所在的真实分支。已推送的分支不 rebase、不 force push。
4. 同一 fork 的多条平台线要合到一个共同分支时用真实 merge，保留每条线的祖先，然后在共同 head 上重跑所有目标的门禁。KrkrCommon63 就是这样把 Linux、Mac、Android 三条 kirikiroid2-web 线合到 `shinki-common63` 的。合流常见冲突：平台专用的静态 host/资源路径被误用到其他平台；同一个 helper 在共享头文件和平台文件里各定义一次。冲突按平台条件收窄，不删功能。
5. 外部核心不是子模块（唯一的子模块是 `ThirdParty/cpal`），用 `build/cores.toml` 固定：

```toml
[cores.source]
repo = "https://github.com/Project-Shinkirou/<repo>"
branch = "<pin 所在分支>"
rev = "<adapter 完整 40 位小写 commit>"
upstream = "https://github.com/<原作者>/<repo>"
release_sequence = 1
upstream_identity = { repo = "https://github.com/<原作者>/<repo>", ref_kind = "tag", ref_name = "<上游 tag>", commit = "<上游完整 commit>" }
```

`CoreSource::validate`（`xtask/src/cores.rs`）要求：`repo` 是组织下的单个仓库；`rev` 是 40 位小写 hex；`upstream` 是原作者的 GitHub 仓库（不能是 Project-Shinkirou），并且等于 `upstream_identity.repo`；manifest 在 `ThirdParty/` 下。上游 commit 有 tag 时 `ref_kind = "tag"`，否则用原上游包含该 commit 的真实分支。身份、序号和 tag 规则见 `shinkiemu-maintenance` §4。写完跑 `cargo xtask release --component <id> --dry-run`，它会真实验证上游 ref 和 adapter 祖先。

## 2. adapter 形态

**Rust 上游：fork 内一个薄 adapter crate。**
- 新建一个 crate（`crates/shinki-emu-<id>` 或 `crates/shinki`），`crate-type = ["rlib", "cdylib"]`；只有原生宿主需要时才加 `"staticlib"`。
- 用 git 依赖 `https://github.com/Project-Shinkirou/ShinkiEMU` 的 `shinki-emu-abi`（需要时加 `shinki-emu-sdk`），`[build-dependencies]` 同样引 `shinki-emu-build`，`rev` 写完整 SHA。在 ShinkiEMU 里，根 `Cargo.toml` 的 `[patch."https://github.com/Project-Shinkirou/ShinkiEMU"]` 会把它们换成树内 crate，所以 fork 里的 pin 只影响 fork 单独构建，但仍要指向真实存在的 ShinkiEMU commit。
- `build.rs` 调用 `CoreBuildManifest::new("<plugin_id>", "rust-cdylib")`，把 fork 的 `LICENSE` 和修改声明列到 `licenses/<plugin_id>/` 下。不支持的 triple 在 `build.rs` 里尽早报错。ReMinori 的 `crates/shinki/build.rs` 可作模板。

**C/C++ 上游：复制头文件 + 原生共享库。**
- 把 `Source/FamilyApi/shinki-emu-abi/include/shinki_emu.h` 复制进 fork（impacto、renpy 放 `vendor/shinki-emu-abi/include/`，kirikiroid2-web 放 `shinki/native/include/`），用 C++ 实现 `CoreApi`。
- fork 里一个小 Cargo 包（`shinki/Cargo.toml` + `build.rs`）通过 `shinki_emu_build::configure_cmake` 驱动 CMake，写 `artifact_kind = "native-shared"` 和绝对 `native_library`；Android 静态嵌入用 `native-static` 和 `link_android_static_core`。
- 升 ABI 时重新复制头文件。目前三个 fork 的头文件相同，但都比产品头文件旧，没有 `CAP_ASYNC_TEXT`/`AsyncTextServiceV1`。这些新增是向后兼容的，旧头文件仍能工作，只是看不到新能力。要用异步文本时先同步头文件。

**动态与静态 query。**
- 桌面按未修饰名 `shinki_emu_query` 加载。移动端 Manager 链接 `rlib` 并调用 Rust 路径；两个核心都导出未修饰 `shinki_emu_query` 会在同一个二进制里冲突。
- 未修饰导出用只在动态构建打开的 feature 门控（rfvp 用 `dynamic-plugin-export`）。这个 feature 写进 `cores.toml` 的 `features`，不能进 `static_features`。
- 给 `STATIC_CORES` 提供 Rust 路径入口：函数本身、`pub const QUERY: Query = shinki_emu_query;`，或具名函数（`artemis_query`、`siglus_rs::query`）。
- `query` 先把输出置空，版本不是 `ABI_VERSION` 时返回 `UNSUPPORTED`，不创建任何原生状态。

## 3. 把上游主循环改成会话

多数引擎自带 `loop { poll events; update; render; present }` 和一个窗口。adapter 要把它改成会话：

- **每个会话一个 worker 线程** 跑上游循环。`open` 启动它，用有界 `recv_timeout` 等 ready 信号，每一步检查取消。sena-rs 的 `session.rs` 是模板：`sync_channel(1)` 握手；`Disconnected` 时 join worker 并返回它记录的错误；取消返回 `CANCELLED`。C++ 里 impacto 的 worker 是 `core.cpp` 的 `Run`。
- **窗口换成离屏输出。** 渲染到离屏目标，回读到不可变 CPU 缓冲，递增 generation 后发布。ReMinori 用三个不可变槽，借出的租约不会卡住 VM 或渲染。`frame` 返回 `CpuLease`（impacto 的 `GetFrame`）。
- **input/control/state** 变成发给 worker 的命令。暂停是停上游时钟，不是阻塞线程。
- **panic 和异常** 不跨 ABI。每个导出和 worker 主体用 `catch_unwind`（Rust）或 `try/catch`（C++）包住，转成带消息的 `SESSION_FAILED`，`close` 仍要 join 并释放所有资源。
- **窗口和模态框** 走宿主的 window/modal 服务，引擎不能自己开。
- 保留上游的 VM、调度、合成、混音、菜单和存档代码，adapter 只做转换。上游需要钩子时（离屏目标、可取消的等待），加最小的通用钩子并尝试提交上游（§9）。

## 4. 存档、key、配置：最小改动

- 外部引擎保持上游自己的存档、key、配置行为、位置和格式，AppData/Documents 也一样，不要重定向到 `.shinki`。只有自研引擎（CMVS、Musica）用 `<game>/.shinki/<core>/`。
- adapter 自己在游戏目录写的文件放 `<game>/.shinki/` 下。
- 不要加托管存档根、游戏目录重叠检查、必填的 `save_root`/`save_directory` 字段或自造的 AppData 命名空间。ReMinori 删掉了 adapter 专用的 `save_directory`，改用上游的 `userdata/` 解析，只是把可执行文件目录换成游戏目录（`ReMinori/MODIFICATIONS.md`，2026-10-29）。
- 收尾前对比 fork base 和上游的存档/key/配置路径，剩下的每处差异都在修改声明里写明原因。
- 删配置字段是一次迁移：未知字段 id 的存储值会导致启动失败，Manager 会清理退役 id。旧的 adapter 专用路径迁移一次（不覆盖已有文件），之后只读上游路径。

## 5. 原生依赖

- 在 `cores.toml` 的 `dependencies` 里用现有 `deps` 配方声明原生输入，配方名在 `xtask/src/dependencies.rs` 解析：`subtitle`、`ffmpeg`、`dxc`、`native-vcpkg`、`impacto-sources`、`renpy`、`bison`、`gstreamer`（只在 Linux x64 生效）。
- `cargo xtask deps --target <triple> --core <id>` 检出 fork 并准备锁定的原生依赖。`build.rs` 只消费和校验，不下载。
- fork 自带的 vcpkg 依赖放在 fork 的 `vcpkg/ports/<port>/`（overlay port），会随 fork 快照进入源码包。修 port 时改 patch 或 portfile，不关配置校验、不伪造 probe 结果（具体案例见 `shinkiemu-maintenance` §6）。
- 只有现有配方表达不了时才加新配方：来源精确固定，许可路由进包。

## 6. 注册与嵌入

- 加 `[[cores]]`：`manifest = "ThirdParty/<dir>/<adapter>/Cargo.toml"`、`[cores.source]`、`licenses = ["ThirdParty/<dir>/LICENSE", ...]`，`build/targets.toml` 里每个 triple 各有一条 supported 或 blocked。字段规则见 `shinkiemu-new-core` §6。
- 移动端静态嵌入三处一致：`cores.toml` 的 `static_manager_targets`/`static_features`；Manager `Cargo.toml` 对应 `cfg` 段写 `git` = `[cores.source].repo`、相同的 `rev`、`default-features = false`；`owner_worker.rs` 的 `STATIC_CORES`。`validate_static_dependencies` 发现 repo、rev、features 不一致时报错。
- 静态核心与 Manager 共享依赖图：kira（根 `[patch.crates-io]` 指向 `Project-Shinkirou/kira`）、mlua/mlua-sys、cpal、wgpu 必须统一。两个版本的 `links =` crate 会让解析失败。改 fork 对齐，不要再加 patch。Artemis 在所有平台都用 Luau，Android 嵌入时共用 Manager 的运行时。
- Android 嵌入 C++ 核心时：Java 方法名不能和 SDK 父类冲突（Krkr 把 `getDeviceId` 改成 `getAndroidDeviceId`），整个 APK 只有一个 `JNI_OnLoad` 和一个 Activity，资源上限遵守 Manager 当前契约。

## 7. 许可证与源码包

- 保留上游 `LICENSE`、署名和 notice。fork 里加修改声明（`MODIFICATIONS.md`），写明上游 URL、base commit、adapter 改了什么和原因、对应源码在哪发布。
- 每个许可文件同时列进 `build.rs`（`licenses/<plugin_id>/…`）和 `cores.toml` 的 `licenses`，并在 `THIRD_PARTY_NOTICES.md`、`ORIGINS.md` 里登记。
- 每个 release 附 `shinki-emu-<slug>-<version>-<platform>-sources.zip`，含发布 commit 的产品树、pin 处的完整 fork 快照，以及许可要求源码分发的 crate 的 `rust-sources/`。只有已提交的内容会进去。打包后确认 fork 文件确实在里面。
- 第三方特殊条款（FMOD、Spine、MS PGothic 等）原文保留并单独说明，不改标成 MIT。
- 许可或声明义务未满足时设 `publish_blocked = "<原因>"`，这样的核心不会发布。
- 商业游戏数据、正文、key、私有配置和存档不能提交。测试用合成 fixture。

## 8. pin 升级

步骤和 `Cargo.lock` 规则见 `shinkiemu-maintenance` §5。要点：

1. fork 改动提交并非 force 推送，取完整 SHA。
2. 文本替换 `build/cores.toml` 的 `rev`（以及静态嵌入的 Manager `Cargo.toml`），`Cargo.lock` 只改该 fork 的 `source = "git+…?rev=<old>#<old>"` 行。
3. 不用 `cargo update`。`git diff -- Cargo.lock` 只有 fork rev 行，`cargo metadata --locked` 成功才算完成。
4. 要发布新版本时 `release_sequence` +1，序号由发布 owner 确认。

## 9. 回馈上游

- 与 ShinkiEMU 无关的修复（VM bug、格式边界、可移植性）从基于 `upstream/main` 的 `upstream/<topic>` 分支提 PR，附合成测试并跑上游自己的检查。
- adapter 专用改动（ABI 胶水、离屏输出、打包）留在 fork，写进修改声明。
- 上游合入后，fork 下次跟进上游时删掉自己的副本。

## 10. 验收

完成标准与 `shinkiemu-new-core` §11 相同：所有支持目标构建和打包成功；干净进程里的 ABI query 探针通过；同一 commit 上 exact-head CI 全绿；源码包和原许可证随包；用 headless runner 打开一款真实游戏并出帧，导出截图和帧 hash。

ABI 探针要在独立原生进程里加载打包后的库，不要用 Python ctypes：Linux 上 Python 的 hashlib 会先加载系统 libcrypto，和核心自带的 OpenSSL 冲突；Windows 上 ctypes 持有 DLL 会让临时目录删不掉。kirikiroid2-web 的共同候选分支（`candidate/krkr-common63`）用 `Tools/krkr-query-probe.c`：用 pin 的 `shinki_emu.h` 编译，`LoadLibraryExW`/`dlopen` 加载后检查 ABI 3 返回完整表、其他版本返回 `UNSUPPORTED`、空输出指针返回 `INVALID_ARGUMENT`。这份 probe 合入 main 之前，其他核心照同样方式写。

## 完成检查

- [ ] fork 保留上游历史，设好 `upstream` remote，base commit 已记录；`branch` 是 pin 所在的真实分支。
- [ ] `[cores.source]` 含完整 SHA、`upstream_identity`、`release_sequence`；`release --dry-run` 通过。
- [ ] 静态嵌入的 Manager 依赖用同一 SHA 和 `static_features`；`STATIC_CORES` 已更新。
- [ ] adapter 保留上游 VM、渲染、音频和存档，只加 ABI 胶水和最小钩子。
- [ ] 未修饰 `shinki_emu_query` 只在动态构建导出；静态构建有 Rust 路径 query。
- [ ] C/C++：复制的 `shinki_emu.h` 覆盖所用的 ABI 能力。
- [ ] panic/异常被拦住；`open` 可取消；`close` join worker；借出的租约不阻塞推进。
- [ ] 存档/key/配置路径与上游一致，差异在修改声明里说明。
- [ ] 许可列在 `build.rs`、`cores.toml`、`THIRD_PARTY_NOTICES.md`、`ORIGINS.md`；源码包含完整 fork 快照。
- [ ] `Cargo.lock` diff 只有 fork rev 行。
- [ ] `docs/{zh-cn,en,ja}/cores/<slug>.md` 链接上游项目；README 引擎表已更新。
- [ ] §10 验收全部满足。
