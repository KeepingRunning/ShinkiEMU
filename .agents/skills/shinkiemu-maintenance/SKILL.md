---
name: shinkiemu-maintenance
description: Use when maintaining ShinkiEMU itself rather than writing a core - xtask doctor/deps/build/package/release, build/cores.toml and targets.toml edits, external-core version identity (upstream_identity, release_sequence, canonical core tags), fork pin bumps and Cargo.lock discipline, vcpkg baseline/overlay triplets/overlay ports, Android/iOS static Manager embedding, corresponding-source and license bundles, GitHub CI exact-head runs and watchers, release/catalog publication, signing (Android release certificate, Apple ad-hoc, OHOS test PKI), native macOS builds over SSH and Mac disk capacity, and headless runs (shinki-emu-headless run/compare/ui, shinki-emu-manager headless/headless-ui) for frame acceptance, screenshot and hash baselines, input replay scripts and windowless Manager UI screenshots. Not for writing a new core (shinkiemu-new-core) or porting a third-party engine (shinkiemu-integrate-existing-core).
---

# ShinkiEMU 项目维护

本 skill 记录维护 ShinkiEMU 时的实际流程和踩过的坑。契约以仓库文档为准，这里只写操作顺序、检查点和容易出错的地方：

- [Build and release](../../../docs/en/build-and-release.md)（中文 `docs/zh-cn/`，日文 `docs/ja/`）
- [Core ABI](../../../docs/en/core-abi.md)、[Metadata](../../../docs/en/metadata.md)
- [xtask/README](../../../xtask/README.md)、[ThirdParty/README](../../../ThirdParty/README.md)

写新核心看 `shinkiemu-new-core`，移植第三方引擎看 `shinkiemu-integrate-existing-core`。

## 1. 权威文件

| 文件 | 管什么 |
|---|---|
| `build/cores.toml` | 每个核心的 plugin_id、manifest、features、`dependencies` 配方、许可输入、`supported_targets`/`[[cores.blocked]]`、`static_manager_targets`/`static_features`、`publish_blocked`；外部核心的 `[cores.source]`（repo/branch/rev/upstream/`upstream_identity`/`release_sequence`） |
| `build/targets.toml` | 10 个 triple、runner、`manager_supported`；target 级 `blocked` 覆盖所有组件 |
| `build/dependencies.toml` | 工具版本和下载输入（SHA256 固定） |
| `build/vcpkg/` | `ffmpeg/`、`subtitle/` manifest；`apple-triplets/`（arm64/x64-osx、arm64-ios、arm64-ios-simulator、x64-ios）；`mobile-triplets/`（`*-android-ffmpeg`、`arm64-ohos`、`ohos-toolchain.cmake`） |
| `Source/Manager/shinki-emu-manager/Cargo.toml` | 静态核心依赖，分 `cfg(target_env = "ohos")`、`cfg(target_os = "ios")`、`cfg(target_os = "android")` 三段 |
| `Source/Manager/shinki-emu-manager/src/manager_app/owner_worker.rs` | `STATIC_CORES` 表，`register_static` 注册 |
| 根 `Cargo.toml` | `[patch."https://github.com/Project-Shinkirou/ShinkiEMU"]` 把 fork 里 git 依赖的 `shinki-emu-abi/build/sdk` 重定向到树内；`[patch.crates-io]` 固定 kira、cpal、ffmpeg-sys-next、mlua-sys |

`Inventory::load`（`xtask/src/cores.rs`）在每条命令开头校验清单：每个 triple 在 supported 或 blocked 中恰好出现一次；`ThirdParty/` 下的 manifest 必须有 `[cores.source]`，树内核心不能有；slug 不能重复；外部 plugin slug 不超过 32 字节；`validate_static_dependencies` 要求 Manager 的 git/rev/path/features 与清单逐项一致，`default-features` 不能为 true。改清单后先跑 `cargo xtask matrix`，它会直接报出违反的规则。

文档改动要三语同步：`docs/{zh-cn,en,ja}/` 同名文件一起改。

## 2. 工作区与隔离

多个 agent 并行工作时，每个 slice 用自己的 worktree 和输出目录，避免互相踩 Cargo lock 和 native 缓存：

```text
worktree       D:/Workspace/Shinkirou-wt/<slice>                 （产品）
fork worktree  D:/Workspace/Shinkirou-wt/forks/<fork>-<slice>
CARGO_HOME     D:/Workspace/.shinki-cargo-home                    （共享）
CARGO_TARGET_DIR      E:/shinki-targets/<slice>
SHINKI_EMU_TARGET_ROOT E:/t/<slice>                               （Windows 短路径根）
TEMP/TMP       E:/shinki-tmp/<slice>
证据与发布实物  E:/shinki-release/<owner>/
```

- 新 worktree 从最新公开 `origin/main` 开：`git worktree add -b <branch> D:/Workspace/Shinkirou-wt/<slice> origin/main`。
- `ThirdParty/*`（除 `README.md`、`cpal/`）被 `.gitignore` 忽略，不是子模块。`cargo xtask deps --core <id>` 会 clone/fetch fork 并 checkout 到 pin；本地有改动时拒绝执行，不会替你 reset。
- 调试未提交的 adapter 改动时用 `deps --allow-dirty-sources`，前提是 fork HEAD 已经是清单 pin。构建记录会标出 dirty，`release` 拒绝 dirty 记录。
- 同一时刻 Mac 只有一个 native slot，`main`、tag、catalog 只有一个发布 owner。没被指派时不要 push `main`、打 tag 或开 catalog PR。

## 3. 构建命令

```sh
cargo xtask matrix                      # 所有组合及 blocked 原因
cargo xtask doctor --target <triple> [--core <id>]
cargo xtask deps   --target <triple> [--core <id> | --all-cores] [--allow-dirty-sources]
cargo xtask build  --target <triple> (--manager | --core <id> | --all-cores) --release
cargo xtask check  --target <triple> (--manager | --core <id>)
cargo xtask test   [--manager | --core <id>]   # 宿主运行，无 --target
cargo xtask package --target <triple> --core <id> --out <新目录>      # 外部核心不传 --version
cargo xtask package --target <triple> --manager --version <semver> --out <新目录>
cargo xtask ci --target <triple> --component <id|manager> --report .work/<name>.json
cargo xtask metadata validate
```

- `doctor` 只诊断，不安装；`deps` 是唯一的下载入口；普通命令不会隐式准备依赖。
- 各组件构建到 `.work/target/<target>/<component>`，构建记录在 `.work/build/<target>/<component>.json`。xtask 从 Cargo JSON 读真实产物路径。
- Windows 长路径会落到 `.work/t/<hash>`，路径仍超长时设 `SHINKI_EMU_TARGET_ROOT`。
- 不要做 workspace-wide all-features 构建。各 fork 有自己的 lockfile 和依赖图。
- CI 的 tools job 跑的是 `cargo fmt --check`、`cargo clippy --locked -p xtask -p shinki-emu-build --all-targets -- -D warnings`、`cargo test --locked -p xtask -p shinki-emu-build`、`cargo xtask metadata validate`。本地改 xtask 后跑同一组即可。

平台优先级与顺序：Windows x64 → Linux x64 → macOS（arm64、x64）→ Android（Manager 静态嵌入）→ iOS（Manager 静态嵌入）→ OHOS（local-only，DevEco 本机打包）。

## 4. 外部核心版本身份

外部核心不用 SemVer。版本从 `[cores.source]` 推导：

```toml
[cores.source]
repo = "https://github.com/Project-Shinkirou/<repo>"
branch = "<fork 上真实存在的分支，如 shinki-android-ports>"
rev = "<adapter 完整 40 位小写 commit>"
upstream = "https://github.com/<原作者>/<repo>"
release_sequence = <正整数>
upstream_identity = { repo = "<与 upstream 相同>", ref_kind = "tag|branch", ref_name = "<原上游 ref>", commit = "<原上游完整 commit>" }
```

- `upstream_identity` 记录原上游。上游 commit 上有 tag 时用 `ref_kind = "tag"`；否则用原上游真实存在、历史包含该 commit 的分支。组织 fork 的分支只写在 `branch`，不能当 upstream。`upstream` 不能是 Project-Shinkirou。
- digest 是 8 个字段（upstream repo/kind/ref/commit、adapter repo/branch/commit、十进制 sequence）各接一个 LF 后的完整 SHA256。版本为 `<tag|branch>-<ref slug>-<digest>`，tag 为 `core-<plugin slug>-<version>`。只改 adapter 也会得到新身份。
- `release_sequence` 每个插件全局递增，跨平台共用一个序号。adapter/upstream pin 变了或要重新发布时 +1；同一序号不能对应两个不同版本，也不能回退。多个候选并行时，序号由发布 owner 统一预留，slice 不要自己占号。
- 改完先跑 `cargo xtask release --component <id> --dry-run`。它会真实 fetch 原上游 ref，验证上游 commit 和 adapter 祖先关系，打印 canonical 版本和 tag，不构建也不发布。
- Manager、Musica、CMVS 仍用 `manager-v<semver>` / `core-<slug>-v<semver>`，`package`/`release` 必须传 `--version`。给外部核心传 `--version` 会被拒绝。
- 已公开的 tag 和资产不可改写。修复只能走新 sequence 的新 release。

## 5. fork pin 升级

1. fork 改动在 fork 分支提交并 push（非 force），取完整 SHA。
2. 在 `build/cores.toml` 里改 `rev`（必要时改 `branch`、`release_sequence`）。如果该核心静态嵌入，同步改 Manager `Cargo.toml` 对应 `cfg` 段里的 `rev`，三段（android/ios/ohos）凡有该核心都要改。
3. `Cargo.lock` 只做文本替换：把该 fork 的 `source = "git+https://github.com/Project-Shinkirou/<repo>?rev=<old>#<old>"` 换成新 SHA。不要 `cargo update`，也不要 `cargo update -p`，它们会顺带漂移无关的 registry 版本（wgpu-hal 的 `windows` 版本就这样弄坏过 D3D12）。
4. 验收条件：`git diff -- Cargo.lock` 只有该 fork 的 rev 行；`cargo metadata --locked` 成功；`cargo xtask matrix` 不报静态 pin 不一致。
5. Android 和 iOS 的静态核心共享 Manager 的依赖图：kira、cpal、mlua/mlua-sys、wgpu、naga 必须能统一到一个版本。两个版本的 `links =` crate 会让解析失败。优先改 fork 对齐，不要再加 patch。iOS 需要同时开 naga 26 的 `termcolor` 特性，原因见 Manager `Cargo.toml` 注释。

同一 fork 的多条平台线（例如 Linux、Mac、Android 各自修过 kirikiroid2-web）要用真实 merge 合到一个共同分支，保留每条线的祖先。不要 squash，不要 rebase 已推送的分支。合流后必须在共同 head 重跑全部目标的门禁，旧分支的通过记录不能挪用。

## 6. vcpkg 与原生依赖

- vcpkg baseline 以 `docs/en/build-and-release.md`「Inputs and prerequisites」里记载的 commit 为准，升级 baseline 时三语文档一起改。registry 必须完整 clone，xtask 需要历史 port tree，不能浅克隆。
- 原生缓存在 `.work/vcpkg-binary-cache`、`.work/vcpkg-downloads`、`.work/tmp`。下载不稳定时可以把同一版本的源码包预放入 `.work/vcpkg-downloads`，vcpkg 仍会按 port 的 SHA512 校验。
- 每个核心、每个 target 用独立的 `installed-<triplet>` 根。Windows、Android arm64、Android x64 先后执行 `deps` 不会互相覆盖。
- Apple overlay triplet 固定 macOS 13.0 / iOS 16.0，并让 libass 用 CoreText。`deps` 会清掉 `*_DEPLOYMENT_TARGET`，避免 macOS 构建机上的 host 工具被编成 iOS 目标。
- 修原生依赖时改 overlay port 或 patch（例如 kirikiroid2-web fork 的 `vcpkg/ports/<port>/`），保持配置校验开启。AppleIos64 的经验：
  - GLib 工具链接缺 CoreFoundation：给 `libintl.patch` 加真实的 Meson `appleframeworks` 依赖，而不是手塞链接参数。
  - libgdiplus 在 iOS 编到 Carbon：用 OSX 宏把 Carbon 代码限定在 macOS，iOS 走原有的 Cairo/Pango 路径。
  - OpenCV 在 AppleClang 上找不到 OpenMP：新增只构建 CPU libomp 的 `llvm-openmp` overlay port（官方 `openmp-18.1.6.src` + `cmake-18.1.6.src`），由真实 FindOpenMP 发现。不要强设 `OpenMP_FOUND`、伪造 probe 结果或关掉 `ENABLE_CONFIG_VERIFICATION`。
  - Autoconf 在 iOS 上把 host/build 判成相同，会去执行 iOS conftest。用 triplet 修正 host 三元组，不要恢复强制 probe cache。
- 构建失败时先读真实日志里的第一个错误，修完再跑下一轮。一轮只修一个首错，修复提交后再开新 run。

## 7. 静态嵌入（Android / iOS / OHOS）

移动端 Manager 不加载下载的动态库。嵌入一个核心要同时改三处，`validate_static_dependencies` 会比对前两处：

1. `build/cores.toml`：`static_manager_targets` 列出 triple，`static_features` 列出静态构建用的 feature；动态导出 feature（`dynamic-plugin-export` 等）不能出现在这里。
2. Manager `Cargo.toml` 对应 `cfg` 段：外部核心写 `git` + 与清单相同的 `rev`，树内核心写 `path`；`default-features = false`，`features` 与 `static_features` 完全一致。
3. `owner_worker.rs` 的 `STATIC_CORES`：加 `#[cfg(...)] ("<plugin_id>", <crate>::<rust-path query>)`。

打包会写 `embedded-cores.json` 和每个嵌入核心的许可文件。Android 常见坑：

- 嵌入的 Java 代码与 SDK 父类同名方法冲突。Krkr 的静态 `getDeviceId(): String` 与 `ContextWrapper.getDeviceId(): int` 冲突，改名为 `getAndroidDeviceId`，独立 JNI 查找保留原名。
- 多核心 APK 只能有一个 `JNI_OnLoad` 和一个 Activity。
- 资源和字体按 Manager 当前的资源上限契约打包（beta7 线为 32 MiB 有界流式），不要把旧分支的更小上限合回来。
- Android 正式安装包必须用发布证书签名，证书 SHA256 为 `c415bd462d26574758ed455055ff528a3bd6036e39d5ac6887308f8cf69e97fc`。签名材料从 `SHINKI_EMU_ANDROID_KEYSTORE`（或 `_KEYSTORE_BASE64`）、`_KEYSTORE_PASSWORD`、`_KEY_ALIAS`、`_KEY_PASSWORD` 读取，CI 从 secrets 注入。没有这些时产物是 `*-debug-signed.apk`，只能当 CI 测试包。

## 8. 源码包与许可证

每个 release 附 `shinki-emu-<slug>-<version>-<platform>-sources.zip`（含 `source.tar` 和 `BUILD-FROM-SOURCE.md`）。验收源码包时检查：

- `source.tar` 里确实有 `ThirdParty/<dir>/` 的完整 fork 快照，版本是 pin 的 commit。KrkrApkFinish61 曾遇到源码包里 kirikiri2 文件数为 0 的情况，起因是 static target 没走 whole-fork 归档规则。
- 原生依赖覆盖 `vcpkg/status` 里所有 port，包括 host 工具；overlay port 在 fork 快照里。
- 上游 `LICENSE`、notice、`MODIFICATIONS.md` 原样保留。不要把第三方许可改标成 MIT。FMOD、Spine、MS PGothic 等特殊条款分开记录。
- Git LFS 指针要先 `git lfs checkout` 成真实内容，否则打包失败。

`publish_blocked` 非空的核心可以编译，但二进制不会上传。

## 9. CI 与 watcher

`ci.yml` 在 push（main、`release/**`）、PR 和 `workflow_dispatch` 时运行。dispatch 输入有 `since`（只跑受影响行）、`components`、`targets`。它的 concurrency 是 `ci-${{ github.ref }}` 且 `cancel-in-progress: true`：同一 ref 的新 push 会取消正在跑的 run。

做 exact-head 门禁时：

1. 为要验收的 commit 推一个专用、不再移动的 ref（例如 `candidate/<slice>-<gate>`），在这个 ref 上 dispatch：
   `gh workflow run ci.yml -R Project-Shinkirou/ShinkiEMU --ref <ref> -f components=<id> -f targets=<t1,t2>`
2. 用 `gh run view <run> -R Project-Shinkirou/ShinkiEMU --json headSha,status,conclusion,jobs` 确认 `headSha` 是你要验收的 commit。
3. 每个 run 只挂一个 watcher：`gh run watch <run> -R Project-Shinkirou/ShinkiEMU --exit-status --interval 60`，作为后台任务等它自然结束，不轮询。
4. 失败时读 `gh run view --job <job> -R Project-Shinkirou/ShinkiEMU --log-failed`，找第一个真实错误。
5. 接手别人的 run 时只查一次状态，需要时再挂唯一 watcher。同一源码已经有 run 时不要重复 dispatch。被新 head 取代的 run 先保存日志和 artifact 元数据，经 owner 同意再取消。

`release.yml` 在推 `manager-v*` / `core-*` tag 时运行，也可以 dispatch：`version`（只用于 Manager/自研核心）、`components`、`prepare_only`、`source_commit`（仅 prepare-only 可用）、`verify_local_manager`（OHOS 交接校验）。一次推超过三个新 tag 不会触发工作流。`prepare_only=true` 只准备并保留各平台产物，不公开；dispatch 所在的 SHA 只代表编排，产品来源以 `source_commit` 为准。

## 10. 发布

发布 owner 在干净 checkout 中按下面顺序操作：

1. 所有目标的 CI 在同一产品 commit 上成功。所有平台的 `release.json` 指向同一个产品 commit 和同一个核心 pin。
2. 把 prepare 产物下载到 `.work/release/<tag>/<target>/`：`gh run download <run> -R Project-Shinkirou/ShinkiEMU -p '<artifact 前缀>*' -D <scratch>`，再按 tag/target 复制。
3. 公开：
   ```sh
   SHINKI_EMU_RELEASE_CI_RUN_ID=<成功的 CI run> GH_TOKEN=<token> cargo xtask release --component <id> --publish            # 外部核心
   SHINKI_EMU_RELEASE_CI_RUN_ID=<run> GH_TOKEN=<token> cargo xtask release --component manager --version <semver> --publish
   ```
   含 OHOS 的 Manager 还需要先建 draft、上传 HAP/源码/`ohos-handoff.json`，dispatch `verify_local_manager=true`，再加 `SHINKI_EMU_RELEASE_HANDOFF_RUN_ID`。
4. 核心公开后，xtask 把每个平台的 catalog 条目提交到 `metadata/release-<tag>` 并开 PR 到 main（`Metadata/engines/cores/<plugin_id>/<platform>/<version>.json`）。用真实 merge 合入，不要 squash。
5. 公开后匿名 HEAD 每个资产，确认 HTTP 200，并核对 API digest/size 与本地记录一致。不需要重复整包下载。

签名边界要在 release 说明里如实写明：Windows 包未签名；macOS/iOS 用 ad-hoc 签名，iOS 是测试产物；OHOS HAP 用 SDK 的 debug test PKI；Android 必须是 c415 发布证书。

## 11. 验收范围

常规门禁必须做：真实构建、既有 CI 全绿、平台正常签名和正确证书、源码与原许可证随包、版本身份不串源（产品 commit、核心 pin、序号互相对应）、ABI query 探针、headless 出帧（见 §13）。

用户已豁免、默认不做：逐 Git blob 或逐输入的来源追溯、签名逆变换和逐字节复原、全资源 census、为补证明而重建、重复下载大资产。豁免的项目在报告里写 "user-waived / not executed"，不要写成通过。实体设备、模拟器、真实账号上的启动验收也属于豁免，除非任务另有要求。桌面 headless 打开真实游戏并出帧不在豁免范围内，新核心必须做。

## 12. Mac 原生构建与容量

- 物理 Mac 通过 SSH 别名 `apple-macos` 访问，Windows 侧用 `C:/Users/wzh20/.agents/skills/ssh-skill/scripts/ssh_execute.py --timeout 21600 --no-daemon` 执行长任务。
- 驱动脚本 `~/shinkirou/run-apple-resume62.sh <product-sha> <core> macos|ios [<single-target>]`。它持有真实锁 `/Users/wzh/shinkirou/locks/mac-build`，每个 target 行开始前检查剩余空间不少于 12 GiB，不会自动删目录。需要 `LIBTOOLIZE=/opt/local/bin/glibtoolize` 和 Bison 3.8.2（系统自带 2.3 太旧）。
- 结果以 `/Users/wzh/shinkirou/logs/apple-ports-<core>-<sha7>-<os>-<target>/summary.txt` 和对应行日志为准。SSH 包装进程退出码 0 不代表构建成功。
- 一次只跑一个 native 任务。开始前确认锁不存在、没有 cargo/rustc/xtask/vcpkg/clang/cmake 进程。
- 空间不足 12 GiB 时停下，把实际阶段和剩余字节报给 Main。不要删 Cargo target、installed 树或下载缓存来腾空间。清理走 managed skill `shinkiemu-expired-build-artifact-cleanup`（Windows）和 `macos-build-artifact-archival`（Mac），每次都要明确授权。
- Mac 上沿用既有隔离根：`CARGO_TARGET_DIR=/Users/wzh/shinkirou/targets/apple-xplat-xtask`、`SHINKI_EMU_TARGET_ROOT=/Users/wzh/shinkirou/targets/apple-xplat-<target>`、cargo home `/Users/wzh/shinkirou/cargo`、rustup `/Users/wzh/shinkirou/rustup`。

## 13. Headless 验收与调试

headless runner 是 crate `Source/Programs/shinki-emu-headless`。完整契约写在 [Headless execution](../../../docs/en/headless.md)，本节只讲验收时怎么用。它复用现有通用层，没有另建协议：

- 核心加载和会话走 Manager 同一套 `shinki-emu-manager-core`（C ABI v3 loader、原生会话 owner、不可变 CPU 帧租约）。跑核心时不创建 Slint 窗口、显示表面或事件循环。
- 输入脚本沿用 RoukakuEngine `roukaku-headless-protocol` 的 `roukaku.user_input_sequence.v1`，用的是工作区已经 pin 的 Engine revision。

入口：

```sh
shinki-emu-headless run     <参数>      # 动态核心：--core 写 core ZIP、解开的核心目录或库路径
shinki-emu-manager  headless <参数>     # Manager 内嵌静态核心：--core 写 plugin_id（来自 static_cores::STATIC_CORES）
shinki-emu-headless compare --actual a.png --baseline b.png [--max-channel-delta N --changed-pixel-ratio R]
shinki-emu-headless ui --page library|cores|patches|settings --language zh-CN|en|ja --theme dark|light --output <dir>   # Manager: headless-ui
```

构建：`cargo build -p shinki-emu-headless`。`--features static-cmvs` 会把 CMVS 以 `shinki.emu.cmvs` 静态链进去；`--no-default-features` 只构建核心 runner，不含 Manager UI。

`run` 的主要参数：

| 参数 | 作用 |
|---|---|
| `--core` | core ZIP / 核心目录 / 库路径，或宿主已链接的 plugin_id |
| `--game-dir` | 源游戏目录。核心拿到路径之前，整个目录先复制到唯一的临时目录，存档和相对路径配置都写在副本里。symlink 和特殊文件会被拒绝；大游戏会占磁盘和时间 |
| `--frames`（默认 60）、`--duration-ms` | 时间线长度，取两者和脚本结束点中的最大值 |
| `--capture-frame`、`--capture-ms` | 截图时间点，逗号分隔；采样号从 1 开始，毫秒向上取整到下一个采样。最后一个采样总会截图 |
| `--output` | 输出目录 |
| `--baseline <目录>` | 与目录中同名 PNG 比较；`--max-channel-delta`、`--changed-pixel-ratio` 给容差，都为 0 时精确比较，尺寸必须一致 |
| `--baseline-hash sha256:<hex>` | 断言最后一帧的规范像素 hash |
| `--script <file>` | 输入脚本 |
| `--configuration <file>` | Manager `ConfigEntry` 数组，例如 `[{"id":"profile_path","value":{"String":"key.toml"}}]`，字段 id 用核心实际声明的 |
| `--startup-timeout-ms`（默认 30000） | native open 之后等第一帧完成的上限 |

**时钟的含义。** C ABI v3 下游戏时钟归核心所有，没有确定性的单步推进。runner 从第一帧完成后开始，按 16 666 667 ns 的主机时间线采样。`--frames N` 是 N 个主机采样点，不是 N 个 VM tick。`acquired_frames` 是实际取到的不同帧数；静态标题画面在多个采样点相同是正常的。两次运行逐像素一致取决于核心和游戏自身的时序，不能宣称 native stepping 是确定的；需要时用容差比较。

**输出。** 每个截图是 `<id>.png` 加 `<id>.json`。sidecar 记录尺寸、采样号、`pixel_hash`、`png_hash` 和比较结果。成功时写 `run.json`，失败时写 `failure.json`（诊断信息和退出码）。比较失败也会保留实际 PNG 和比较 sidecar。`pixel_hash` 是规范像素的 SHA256（加 `sha256:` 前缀）：自上而下、紧密排列的 RGBA8 sRGB，alpha 固定 255，BGRA、带 padding 的 stride、自下而上的行序和 linear 格式都先归一化。所以 PNG 编码差异不影响它。

**退出码。** 0 成功；1 运行、输入、文件、ABI 错误或没出帧；2 命令行语法错误（仅独立二进制）；3 基线或回放断言失败。

**脚本。** JSON 数组，每项一条 `InputMessage`：`sequence` 严格递增，`tick` 不减，`session` 不变。指针坐标是归一化的 0..65535，映射到核心的逻辑画面。一次点击是 move + press + release，按键的按下和抬起都要显式写：

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

- 支持的事件：`resume`、`focus`、`keyboard`、`ime_commit`、`pointer_move`、`pointer_button`、`wheel`、`advance_ticks`、`await`、`checkpoint`。其他协议事件会明确报错。
- 按键名用 Manager 的物理键名（`KeyA`、`Enter`、`ControlLeft`、`ArrowDown`、`F1` 等）。
- `advance_ticks` 推迟后续消息，不影响已排好的截图。
- `await` 只支持 `{"kind":"exists","key":"frame"}` 和 `{"kind":"equals","key":"frame.sha256","value_hash":"sha256:<64 hex>"}`；超过 `timeout_ticks` 未满足即断言失败。`continue_at_match` 必须为 false，因为核心时钟无法回退。
- `checkpoint` 导出 `<id>.png`，并与 `--baseline` 里的同名文件比较。id 是唯一的扁平文件名，`frame-` 前缀留给定时截图。

**常见用法。**

1. **新核心出帧验收**（`shinkiemu-new-core` §11 第 5 项）：`run --core <打包好的 ZIP> --game-dir <真实游戏> --frames 600 --capture-frame 120,600 --output .work/headless/<slug>`。要求 `run.json` 存在、`acquired_frames > 0`，并打开 PNG 确认是游戏画面而不是全黑或纯色。把 `run.json` 和 PNG 路径写进报告。静态嵌入的核心再用 `shinki-emu-manager headless --core <plugin_id>` 跑一次。
2. **回归对比**：把第一次运行的输出目录作为 `--baseline`。GPU 合成可能有细微差异，给小容差，例如 `--max-channel-delta 4 --changed-pixel-ratio 0.001`。单张图用 `compare`。
3. **流程回放**：在关键步骤后放 `checkpoint`，用 `await` + `frame.sha256` 断言到达已知画面。
4. **Manager UI 截图**：`ui` / `headless-ui` 用真实 `ManagerWindow` 经软件渲染出图，不创建窗口，不启动数据库、库扫描、账号、商店或网络服务。默认数据是自写的合成文本；`--demo-data` 只提供显示字符串，不会被扫描。不同主题用不同输出目录。

**限制。** 复制游戏只是隔离普通存储，不是给不可信核心用的系统沙箱。配置里故意指向游戏外的路径，要改到临时位置。用 `E:/Games` 下的游戏时原目录同样只读。核心自己需要的 GPU 或音频设备照样需要；runner 不会关掉音频、影片或脚本。Musica、CMVS 这类硬件 GPU 合成的核心，软件 adapter 的结果不算通过。Windows x64 是首个支持平台，其他平台的状态以 HeadlessRunner 的交付报告为准。

**调试。** 先用 runner 复现问题，再看核心的 `tracing` 事件（`shinki.emu.<family>.*`）。核心 `SESSION_FAILED` 时，错误码和消息会写进 stderr 和 `failure.json`。Musica 另有只读诊断 CLI `shinki-emu-musica-cli data list|read|validate`（见 `Source/Programs/shinki-emu-musica-cli/README.md`），它不运行 VM。

## 完成检查

- [ ] `cargo xtask matrix` 通过；文档三语同步。
- [ ] 外部核心 `release --dry-run` 打印的 tag 符合预期；序号由发布 owner 确认。
- [ ] `Cargo.lock` diff 只含 fork rev 行；静态嵌入三处一致。
- [ ] 每个支持目标在同一 exact head 上 CI 成功，并且只有一个 watcher。
- [ ] 源码包包含完整 fork 快照和原许可证；签名边界写进 release 说明。
- [ ] 公开 tag/资产没有改写；catalog 用真实 merge 合入。
