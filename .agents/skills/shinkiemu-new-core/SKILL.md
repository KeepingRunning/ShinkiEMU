---
name: shinkiemu-new-core
description: Use when writing a new ShinkiEMU core from scratch on the in-repo Rust SDK (a crate under Source/Families/ implementing C ABI v3 with shinki-emu-sdk), or when choosing between writing one and porting a third-party engine. Covers the port-vs-SDK decision, crate layout, query/CoreApi table, worker and CPU frame leases, which SDK module to use (runtime, native, native_input, lease_cpu, text, image, audio, device-audio, subtitle, video-ffmpeg, archive, cache, profile), core-build.json, build/cores.toml registration, static embedding, tests, docs, release tags and the completion standard (build, ABI probe, exact-head CI, sources/licenses, real game opens and renders headless). Not for wrapping an existing third-party engine; use shinkiemu-integrate-existing-core.
---

# 基于 SDK 编写新的 ShinkiEMU 核心

参考实现是 CMVS（`Source/Families/shinki-emu-cmvs`），Musica（`Source/Families/shinki-emu-musica`）是第二个例子。照着它们的结构写，不要另起一套约定。

契约文档直接链接，不要抄进核心：[Core ABI](../../../docs/en/core-abi.md)、[Build and release](../../../docs/en/build-and-release.md)、[Metadata](../../../docs/en/metadata.md)、[SDK README](../../../Source/SDK/shinki-emu-sdk/README.md)。构建、CI、发布流程见 `shinkiemu-maintenance`。

## 0. 先选路径

| 情况 | 路径 |
|---|---|
| 有可运行的上游实现，许可证允许再分发（MPL、MIT、Apache、GPL/AGPL 且能履行源码义务） | 移植第三方 fork，用 `shinkiemu-integrate-existing-core` |
| 上游存在但许可证不兼容、无法分发，或代码无法在离屏/多平台下运行 | 用本 skill 基于 SDK 自研 |
| 没有上游实现，只有原版可执行文件 | 用本 skill，逆向原版行为后自研（CMVS、Musica 都是这样做的） |
| 上游只覆盖部分格式 | 先移植上游；缺的部分作为 fork 内的 adapter 修改并尽量提交上游，不要在树内再写一个同引擎核心 |

自研核心属于本仓库：plugin_id 用 `shinki.emu.<family>`，没有 `[cores.source]`，版本用 SemVer。

## 1. 命名

- **plugin_id**：`Descriptor.plugin_id`、`core-build.json`、`build/cores.toml` 的 `id`、catalog 和存档共用的稳定标识，格式 `shinki.emu.<family>`。字符限 `[A-Za-z0-9._-]`，最长 128 字节（`CoreBuildManifest::validate`，`Source/Tools/shinki-emu-build/src/lib.rs:54-62`）。发布后不能改名。
- **slug**：由 plugin_id 推导，不手写。小写后把 `.`、`_` 换成 `-`（`shinki.emu.cmvs` → `shinki-emu-cmvs`）。docs 页名、tag `core-<slug>-v<semver>` 和归档名都用它。重复 slug 会被 `Inventory::validate` 拒绝。
- **crate**：`Source/Families/shinki-emu-<family>`，package `shinki-emu-<family>`，加进根 `Cargo.toml` 的 `[workspace] members`。
- **family_id**：descriptor 里的引擎族字符串（`"cmvs"`）。多个核心可以共用一个 family，例如 `ReMinori` 和 `shinki.emu.musica` 都是 `musica`。

## 2. crate 骨架

`Cargo.toml` 参照 `Source/Families/shinki-emu-cmvs/Cargo.toml`：

```toml
[package]
name = "shinki-emu-<family>"
version.workspace = true
edition.workspace = true
license.workspace = true
build = "build.rs"

[lib]
crate-type = ["rlib", "cdylib"]   # cdylib 给桌面动态加载，rlib 给移动端 Manager 静态链接

[dependencies]
shinki-emu-abi = { path = "../../FamilyApi/shinki-emu-abi", features = ["diagnostic-bridge"] }
shinki-emu-sdk = { path = "../../SDK/shinki-emu-sdk", default-features = false, features = ["runtime" /* 只开用到的 */] }

[build-dependencies]
shinki-emu-build = { path = "../../Tools/shinki-emu-build" }
```

`build.rs` 参照 `Source/Families/shinki-emu-cmvs/build.rs`：

```rust
use shinki_emu_build::{BuildAsset, CoreBuildManifest, write_core_build_manifest};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for input in ["HOST", "TARGET", "PROFILE"] {
        println!("cargo:rerun-if-env-changed={input}");
    }
    println!("cargo:rerun-if-changed=src");
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR")?)
        .join("../../..").canonicalize()?;
    let mut manifest = CoreBuildManifest::new("shinki.emu.<family>", "rust-cdylib")?;
    for file in ["LICENSE-MIT", "LICENSE-APACHE"] {
        manifest.licenses.push(BuildAsset { source: root.join(file), dest: format!("licenses/shinki.emu.<family>/{file}") });
    }
    write_core_build_manifest(&manifest)?;
    Ok(())
}
```

`CoreBuildManifest::validate`（`lib.rs:50-105`）的规则：
- `rust-cdylib` 不带 `native_library`；`native-shared` 要求存在的绝对路径；`native-static` 要求 `.a`/`.lib`。
- 资产 source 是存在的绝对路径；dest 以 `Resources/` 或 `licenses/` 开头，不含 `\`、`:`、空段、`.`、`..`。
- 随包的每个许可文件也要列进 `build/cores.toml` 的 `licenses`。

`build.rs` 只校验输入并把 `core-build.json` 写进 `OUT_DIR`。不递归调用 Cargo，不下载，不发布。

## 3. C ABI 入口：`src/native.rs`

按顺序读 CMVS 的这些位置：

| 内容 | 位置 |
|---|---|
| 每个导出外层的 panic/错误边界 | `Source/Families/shinki-emu-cmvs/src/native.rs:28`（`boundary`） |
| 带 `Header` 的静态 `CoreApi` 表 | `native.rs:52` |
| 静态注册用的 Rust 路径 query，和动态加载用的未修饰 `shinki_emu_query` | `native.rs:69`、`native.rs:81` |
| `initialize`、`describe`、`probe`（probe 在返回前 join 自己的 worker） | `native.rs:87-202` |
| `open`：校验请求，启动 worker，等 ready 握手 | `native.rs:203-332` |
| `input`、`control`、`state`：只排队命令，不阻塞在 VM 上 | `native.rs:333-412` |
| `frame`：`shinki_emu_sdk::native::lease_cpu` | `native.rs:413-461` |
| `cancel`（只置标志）和 `close`（join worker） | `native.rs:462-487` |
| descriptor、能力位和配置 schema | `src/native/config.rs:16`（从 `crate::family::cmvs_descriptor().configuration` 生成） |
| 从 `OpenRequest` 解出配置 | `src/native/config.rs:136`（`shinki_emu_sdk::native::config_entries`） |
| worker 主循环 | `src/native/worker.rs:143-243` |

最小形态：

```rust
static API: abi::CoreApi = abi::CoreApi {
    header: abi::Header::of::<abi::CoreApi>(),
    initialize: Some(initialize), describe: Some(describe), probe: Some(probe),
    open: Some(open), input: Some(input), control: Some(control), state: Some(state),
    frame: Some(frame), cancel: Some(cancel), close: Some(close), extension: Some(extension),
};

/// 移动端 Manager 通过 Rust 路径调用。
pub unsafe extern "C" fn <family>_query(version: u32, out: *mut *const abi::CoreApi) -> abi::Status {
    if out.is_null() { return abi::INVALID_ARGUMENT; }
    unsafe { out.write(std::ptr::null()) };
    if version != abi::ABI_VERSION { return abi::UNSUPPORTED; }
    unsafe { out.write(&API) };
    abi::OK
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shinki_emu_query(version: u32, out: *mut *const abi::CoreApi) -> abi::Status {
    unsafe { <family>_query(version, out) }
}
```

`query` 不创建任何原生状态，不做 I/O，不启动线程，不装 logger。每个导出都包在 `boundary` 里，panic 变成 `PANICKED` 加一个 `Error`，绝不跨 `extern "C"` 展开。

## 4. 运行模型

- **worker 拥有引擎。** `open` 启动一个线程，VM、渲染、音频都归它。API 调用通过 channel 发 `Command`。CMVS 的循环（`worker.rs:154` 起）用 `recv_timeout(8ms)` 收命令，处理 control/input，按实际经过的时间推进（暂停时为 0），然后发布一帧。Manager 没有 advance 回调，推进完全由核心自己负责。
- **帧。** 在锁里发布不可变像素缓冲，只有内容变化时递增 `generation`（`worker.rs:231`）。第一帧之前 `frame` 返回 `FRAME_PENDING`，调用方身份相同时返回 `FRAME_UNCHANGED`，否则用 `lease_cpu` 返回 CPU 租约。被借出的帧不能阻塞 VM，也不能被覆盖；会话关闭后租约仍可读、可归还。
- **生命周期。** 状态是 `RUNNING`/`PAUSED`/`FINISHED`/`FAILED`。`cancel` 只设 `ServiceContext` 的取消标志，必须能唤醒阻塞中的原生等待。`close` 无论成败都消耗句柄，join worker 并释放所有媒体/音频资源。worker 清理前调用 `services.invalidate()`（`worker.rs:123`）。
- **窗口/模态请求** 走 `shinki_emu_sdk::native::window_request`（`worker.rs:210`）。核心不能自己开系统窗口。
- **配置。** 字段只在 family descriptor 里声明一次。Manager 会拒绝未知字段 id 的存储值，所以删字段等于一次迁移。
- **日志。** 打开 `shinki-emu-abi/diagnostic-bridge`，用 `tracing` 输出稳定的 `event = "shinki.emu.<family>.<event>"`。正文、私有路径、未审查的 Debug 内容不能进日志。
- **存档。** 自研核心按原引擎的位置和字节格式写原生存档，让原版可执行文件也能读。原引擎放在注册表、AppData、Documents 里的东西放到 `<game>/.shinki/<family>/`（CMVS：`<game>/.shinki/cmvs/registry.json`）。ShinkiEMU 在游戏目录写的其他文件也都放在 `<game>/.shinki/` 下（[Build and release](../../../docs/en/build-and-release.md#game-local-on-disk-layout)）。

## 5. SDK 模块怎么选

SDK 只提供公共机制，不提供游戏运行时。VM、混音规则、媒体时钟、存档和游戏规则都留在核心里。详细 API 见 SDK README。

| 需求 | 模块 / feature | 入口 |
|---|---|---|
| 进程内配置、输入、窗口命令、错误、帧值 | `runtime` | `shinki_emu_sdk::runtime::*`；C ABI 配置先过 `shinki_emu_abi::validate_config` |
| 取消、epoch、宿主服务失效、窗口结果 | `runtime`（`native`） | `native::ServiceContext`（`is_cancelled`/`invalidate`/`cancel`）、`native::config_entries`、`native::window_request`、`native::NativeFailure` |
| CPU 帧租约 | `runtime`（`native`） | `native::lease_cpu`：独立 owner 持有不可变像素和租约计数 |
| 把 C 输入复制成有序事件，指针按舞台逆映射 | `runtime`（`native_input`） | `native_input::translate`；不调用 VM |
| 逻辑舞台到光栅的映射、逻辑/物理纹理分离 | `image` | `StageCanvas`、`TextureAsset` |
| 字体排版和字形资源 | `text` | `TextScene`（`frame` 产出先资源后绘制的 `SceneCommand`）、`TextScene::layout_shared`（只读度量） |
| 有界纹理缓存、场景纹理驻留 | `image` | `TextureCache`、`SceneTextureResidency`（每个 renderer 一个实例，提交前 `prepare`） |
| 完整解码一段音频为浮点 PCM | `audio` | `decode_audio`，借用 `&[u8]`，调用方给最大帧数和取消标志 |
| 核心自己的音频设备线程 | `device-audio` | `NativeAudioOutput`、`NativeAudioConverter`（CPAL；设备错误上报，不切静音后端） |
| ASS 字幕 | `subtitle` | `AssScene`：`update` 产出资源变更，`append_draws` 追加绘制 |
| FFmpeg 视频解码 | `video-ffmpeg` | `VideoFramePacket`（BGRA8 + 实际颜色转换） |
| 归档索引和读取结果 | `archive` | `ArchiveManifest`（`shinki.emu.archive_manifest.v2`） |
| 有界明文磁盘缓存 | `cache` | `CacheIdentity`；是否落盘由核心决定 |
| 游戏目录内或外置私有配置 | 默认 | `read_game_profile`、`read_configured_profile_bytes`、`resolve_game_file` |

渲染失败要关闭对应会话，不能丢掉资源命令继续画。不要再加第二套音频栈、第二套纹理缓存或第二个 renderer。

## 6. 注册核心

在 `build/cores.toml` 加 `[[cores]]`，参照 `shinki.emu.cmvs`：

```toml
[[cores]]
id = "shinki.emu.<family>"
display_name = "Shinki <Family>"
family_name = "<Family>"
license = "MIT OR Apache-2.0"
manifest = "Source/Families/shinki-emu-<family>/Cargo.toml"
package = "shinki-emu-<family>"
features = []
dependencies = []            # deps 配方，例如 "ffmpeg"；纯 Rust 为空
artifact_kind = "rust-cdylib"
resources = []
licenses = ["LICENSE-MIT", "LICENSE-APACHE"]
supported_targets = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "x86_64-apple-darwin", "aarch64-apple-darwin"]
publish_blocked = ""
[[cores.blocked]]
target = "aarch64-linux-android"
reason = "<具体原因>"
# build/targets.toml 里其余每个 triple 各一条 [[cores.blocked]]
```

清单加载时检查：每个 triple 恰好出现在 supported 或 blocked 中一次，blocked 原因非空；树内核心不能有 `[cores.source]`；`publish_blocked` 在许可或声明义务未满足时填写具体原因，这样的核心不会发布。新核心不要改 `build/targets.toml`。

平台按 Windows x64 → Linux x64 → macOS → Android → iOS → OHOS 的顺序推进。blocked 原因写到能定位的程度（缺哪个依赖准备路径、哪个文件哪段代码），不要写"暂不支持"。

## 7. Android / iOS / OHOS 静态嵌入

移动端 Manager 不加载下载的动态库。三处必须一致，`validate_static_dependencies` 会比对前两处：

1. `build/cores.toml`：`static_manager_targets` 列 triple，`static_features` 列静态构建 feature。
2. `Source/Manager/shinki-emu-manager/Cargo.toml` 对应的 `cfg(target_os = "android")` / `cfg(target_os = "ios")` / `cfg(target_env = "ohos")` 段：`path` 依赖，`default-features = false`，`features` 与 `static_features` 完全相同。Musica 是现成例子。
3. `owner_worker.rs` 的 `STATIC_CORES`（`:269` 起）：例如 `("shinki.emu.musica", shinki_emu_musica::musica_query)`。

打包会把 `embedded-cores.json` 和嵌入核心的许可写进 Manager 包。

## 8. 测试

- 单元和行为测试放在代码旁边（CMVS 是 `src/native/tests.rs`），用 `cargo xtask test --core shinki.emu.<family>` 跑。
- `native/tests.rs` 里的 `queued_input_drives_real_session_and_cpu_lease_survives_joined_close` 是 ABI 层行为测试的模板：真实 worker、真实输入、窗口服务替身、关闭后租约仍可读。
- 覆盖使用方看得到的行为：格式和预算边界、损坏或超大的输入、存档往返和完整性、open 期间取消、暂停/恢复、过期帧身份、租约在 worker 结束后仍有效、失败后 worker 正常退出。
- fixture 在测试里生成或提交自制的小文件。商业游戏数据、正文、key、存档都不能进仓库。
- 不要写固定措辞、常量、路径拼写或纯转发的测试。

## 9. 本地构建与打包

```sh
cargo xtask doctor  --target x86_64-pc-windows-msvc --core shinki.emu.<family>
cargo xtask deps    --target x86_64-pc-windows-msvc --core shinki.emu.<family>
cargo xtask build   --target x86_64-pc-windows-msvc --core shinki.emu.<family> --release
cargo xtask check   --target x86_64-pc-windows-msvc --core shinki.emu.<family>
cargo xtask package --target x86_64-pc-windows-msvc --core shinki.emu.<family> --version 0.1.0-beta.1 --out .work/packages/<slug>
cargo xtask matrix
```

缺 `shinki_emu_query` 导出、缺许可、架构不对或原生依赖不闭合时打包失败。要在桌面 Manager 里试跑，用 `cargo xtask build --target <triple> --manager --release` 构建 Manager 后安装核心包。

## 10. 文档、catalog、发布

- 新增 `docs/{zh-cn,en,ja}/cores/<slug>.md`，参照 `docs/en/cores/shinki-emu-cmvs.md`；在各语言 `index.md` 里链接，并在 `README.md`、`README.zh-CN.md`、`README.ja.md` 的引擎表加一行。
- tag `core-<slug>-v<semver>`，推 tag 触发 `release.yml`；发布前在 exact head 上跑通 CI。发布由发布 owner 执行（流程见 `shinkiemu-maintenance` §10），xtask 公开后会自己开 catalog PR。不要给未公开的构建手写 catalog 条目。
- 改了 `Metadata/` 就跑 `cargo xtask metadata validate`。

## 11. 完成标准

新核心满足以下全部条件才算完成：

1. **构建**：所有 supported 目标 `cargo xtask build`/`package` 成功，blocked 目标有具体原因。
2. **ABI 探针**：在干净进程里加载打包后的库，`shinki_emu_query` 对 ABI 3 返回完整表，对其他版本返回 `UNSUPPORTED` 且输出为空，对空输出指针返回 `INVALID_ARGUMENT`。
3. **exact-head CI**：同一个 commit 上 `ci.yml` 的 tools、matrix 和每个支持目标都成功。
4. **源码包与许可证**：`release --target` 生成的 sources ZIP 含产品快照和完整依赖源码，原许可证随包。
5. **真实游戏打开并出帧**：用 headless runner 以只读游戏目录（或副本）打开一款真实游戏，推进到确有画面输出，导出截图和帧 hash（用法见 `shinkiemu-maintenance` 的 headless 一节）。存档写在临时目录。

## 完成检查

- [ ] descriptor、`build.rs`、`cores.toml` 里的 plugin_id 一致，slug 唯一。
- [ ] 每个导出都在 `boundary` 里；`query` 不建状态；`close` join worker。
- [ ] 帧只在变化时递增 `generation`；借出的租约不阻塞 VM。
- [ ] 游戏目录写入都在 `<game>/.shinki/` 下（原格式存档除外）。
- [ ] `cores.toml` 覆盖每个 target；许可与 `build.rs` 一致。
- [ ] 静态嵌入在 `cores.toml`、Manager `Cargo.toml`、`STATIC_CORES` 三处一致。
- [ ] 行为测试用合成 fixture，`cargo xtask test --core <id>` 通过。
- [ ] 三语文档和 README 表已更新；`cargo xtask metadata validate` 通过。
- [ ] §11 的五项完成标准全部满足。
