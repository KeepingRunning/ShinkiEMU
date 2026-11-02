---
title: 构建与发布
description: Cargo 编排、核心构建脚本、打包及明确的 blocked 状态。
---

# 构建与发布

无窗口核心验收、输入回放、PNG/hash 基线比较与 Manager 自身界面截图，见[无窗口运行](headless.md)。

## 游戏目录中的磁盘布局

ShinkiEMU 自身在游戏目录内写入的文件一律放在 `<game>/.shinki/` 下；产品存档、缓存、日志、回执、补丁附件及临时文件不得写进商业游戏根目录或原版存档目录。各组件使用自己的子路径：Musica 存档为 `musica/saves/`，下载附件为 `patch/`，事务暂存为 `staging/`，原文件及清单为 `backup/<id>/`，原位覆盖回执为 `patch-result.json`。增加写入者时沿用安全路径校验，不跟随目录链接。第三方核心继续遵守各自上游的存储约定。

## 输入与依赖

克隆仓库时递归取得子模块，并准备 Rust/Cargo 和目标平台的原生工具链。Windows 原生构建需要 x64 MSVC 开发者环境、CMake、Ninja 和 Git。Android 使用 JDK 27、NDK 30.0.16248370 和 cargo-ndk 4.1.2。工具版本与下载输入在 `build/dependencies.toml`；核心 manifest、features、许可在 `build/cores.toml`；目标与 runner 定义在 `build/targets.toml`。

Ren’Py 另需固定的官方 SDK，设置 `RENPY_SDK_ROOT`、`RENPY_PYTHON_DEV`、`RENPY_SDL_INCLUDE`，并准备与 CPython 3.12.7 匹配的官方 Cython 3.2.4 编译器。这些属于上游编译和运行依赖。SDK 输入以 fork 的 `shinki/sdk.json` 为准。
POSIX SDL 2.0.20 源码从 SDL 官方 GitHub Release 下载，SHA256 仍为 `c56aba1d7b5b0e7e999e4a7698c70b63a3394ff9704b5f6e1c57e0c16f04dd06`，归档内容和 SDK ABI 不变。Android Ren’Py 源码构建还需要 LLVM 18/Clang 18、LLD、ccache 及原生开发包；CI 和 Release 安装同一份既有依赖清单，只有 Android SDK/NDK 不够。


工作区统一使用 Rust 2024 edition，最低 Rust 版本为 1.99，固定工具链为 Rust 1.99.1。Android 打包使用 Gradle 9.8.2、AGP 9.4.1 内置 Kotlin、JDK 27、platform 37.2、build-tools 37.0.0 和稳定版 NDK r30（30.0.16248370）。CMVS 不依赖 Win32，以动态核心构建 Windows x64、Linux x64 和两种 macOS 架构；Android 与 iOS 需等 Manager 静态嵌入。

原生库固定 vcpkg baseline `0c2a9ace1964b818b15f7ae50ba1c42404918a7b`，对应 FFmpeg 9.0.2#2、libass 0.17.5、HarfBuzz 14.6.0、FriBidi 1.0.17、FreeType 2.14.3。CI 使用 NASM 3.02，DXC 固定为 1.9.2609。构建运行在 JDK 27 上，Java/Kotlin 字节码仍以 JVM 17 为目标，保持 Android 兼容性。RoukakuEngine 固定到已发布 revision `374a7a20780153766d9d0256e4cf4d7754fd02f9`，包含 Android 离屏 Vulkan 后端；外部核心来源由独立任务合入。Bangumi/VNDB SDK 的公开 API 仍要求 reqwest 0.12 类型，因此保留最新补丁版 0.12.28，应用自己的 HTTP 请求使用 0.13.5。Ren’Py 的 CPython/Cython/SDL 输入继续匹配其固定 SDK 的 ABI。Android 静态核心通过来源 patch 共用工作区的 ABI 与构建辅助库，不再解析第二份同版本的构建辅助包。

mlua 0.12.2 的 mlua-sys 0.13.0 固定到官方上游修复 `ff96ed786295cae96f43ef1372bd22be0fbf7866`。发布版在 `Lua::new` 中会拒绝当前 Luau 744 生成的字节码；上游修复将识别上限从 11 同步为 14。

Windows 交叉构建使用固定 Engine revision 中的 Android triplet，通过 Windows 扩展绝对路径前缀同时满足 Meson 1.12 的 POSIX 目标路径检查和本机暂存要求。Java 27 的运行支持见 [Gradle 9.8.2 兼容矩阵](https://docs.gradle.org/9.8.2/userguide/compatibility.html)；[Android JDK 指南](https://developer.android.com/build/jdks) 要求 JDK 不低于 AGP 的最低版本（AGP 9.4 为 17）。

从仓库根目录执行：

```sh
cargo xtask --help
cargo xtask matrix
cargo xtask doctor --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc
cargo xtask doctor --target x86_64-pc-windows-msvc --core renpy
```

`doctor` 检查缺失输入，失败时返回非零，不安装依赖。`deps` 是显式下载和准备入口，可用 `--core <plugin_id>` 缩小范围；它准备锁定的原生依赖，不安装系统包。普通命令不会隐式执行 deps，Cargo 仍需能取得已解析的依赖。

## 构建、检查与测试

```sh
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --core rfvp --release
cargo xtask build --target x86_64-pc-windows-msvc --all-cores --release
cargo xtask check --target x86_64-pc-windows-msvc --manager
cargo xtask check --target x86_64-pc-windows-msvc --core rfvp
cargo xtask test --manager
cargo xtask test --core rfvp
cargo xtask test
```

build 必须在 `--manager`、`--core`、`--all-cores` 中选一项。check 选择 Manager 或单个核心；test 在宿主运行，没有 `--target` 参数。不带选择的 test 运行 xtask 和构建辅助库测试。原生行为回归可能需要真实 GPU、音频设备及公开夹具。

每个核心的 `build.rs` 校验本核心的目标和输入，在 Cargo `OUT_DIR` 写入 `core-build.json`。C++ 适配通过它驱动真实 CMake 构建，纯 Rust 核心仍由 Cargo 编译。构建脚本不递归运行 Cargo，也不发布 Release。xtask 从 `compiler-artifact` 和 `build-script-executed` JSON 读取真实产物，不猜测 target 路径。各组件使用 `.work/target/<target>/<component>`，构建记录写入 `.work/build/`。Artemis 在所有平台统一使用 Luau，Android 静态嵌入时与 Manager 共用运行时。

Manager 在创建网络客户端和工作线程前，仅初始化一次进程级 Rustls 加密实现，选择与 reqwest 0.13 和平台证书校验栈一致的 AWS-LC。VNDB 当前使用的 reqwest 0.12 还会启用 ring，不能依靠 feature 自动判断默认实现。Steam 等 provider 自身不安装进程级默认实现。

### 编译进程序的商店插件

`shinki-emu-store` 定义对象安全的 provider/session trait、取消机制、密钥库边界和 `Registry`。Manager 默认启用 `store-steam`、`store-dlsite` 和 `store-dmm`。只编译 Steam 时，对 Manager 包使用 `--no-default-features --features store-steam`。新增商店需要一个 provider crate、对应的可选 Cargo 依赖与 feature，以及 `manager_app/store.rs` 中一行 `registry.register(...)`；不动态加载库。Provider 返回进程内的不透明下载计划、无凭据的固定 `StoreError` 消息，以及账号拥有的作品。临时文件必须使用传入的 `.shinki/store/<商店>/` 工作目录；凭据只通过 `SecretStore` 保存。


### Linux 桌面

使用仓库固定的 Rust 1.99.1 工具链。在 Ubuntu 22.04/24.04 上显式安装原生开发包：

```sh
sudo apt-get install -y --no-install-recommends build-essential cmake ninja-build pkg-config nasm patchelf autoconf automake libtool libltdl-dev autoconf-archive python3-jinja2 python3-venv libxtst-dev libxxf86vm-dev libxinerama-dev bison flex gperf libssl-dev libasound2-dev libudev-dev libx11-dev libx11-xcb-dev libxcb1-dev libxrandr-dev libxi-dev libxcursor-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan-dev mesa-vulkan-drivers xvfb xauth unzip jq
sudo apt-get install -y --no-install-recommends libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libglib2.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav libgtk-3-dev libwebkit2gtk-4.1-dev
```

`libltdl-dev` 为 vcpkg `libxcrypt` 的 autoreconf 步骤提供 `LT_SYS_SYMBOL_USCORE` 宏。kirikiroid2-web 的原生依赖生成还需要 `python3-jinja2`，其 Linux GTK 依赖闭包需要 `libxtst-dev`。这些是构建主机依赖，`cargo xtask deps` 不会安装系统包。
`python3-venv` 为 libsystemd 的隔离构建环境提供 ensurepip，`libxxf86vm-dev` 提供 Linux Cocos 链接所需的 Xxf86vm 库。

```sh
cargo xtask deps --target x86_64-unknown-linux-gnu
cargo xtask deps --target x86_64-unknown-linux-gnu --all-cores
cargo xtask build --target x86_64-unknown-linux-gnu --manager --release
cargo xtask build --target x86_64-unknown-linux-gnu --all-cores --release
cargo xtask package --target x86_64-unknown-linux-gnu --manager --version 0.1.0-beta.1 --out .work/linux-manager
cargo xtask package --target x86_64-unknown-linux-gnu --core siglus_rs --out .work/linux-siglus
cargo xtask ci --target x86_64-unknown-linux-gnu --component manager --report .work/linux-ci.json
```

单独构建 Linux 原生核心时，将 `CORE` 换成 `renpy`、`impacto` 或 `kirikiroid2-web`：

```sh
cargo xtask deps --target x86_64-unknown-linux-gnu --core CORE
cargo xtask ci --target x86_64-unknown-linux-gnu --component CORE --report .work/linux-core-ci.json
```

Ren'Py 核心包带有 SDK 运行库和安装后的 `Resources`；Impacto 与 KiriKiri2 核心包包含锁定版本的原生共享库依赖闭包。每个核心包在 `build.json` 中记录系统 SONAME 需求。

Linux Manager 的 `deps` 检查发行版提供的开发库，不调用 `apt`。核心的 `deps` 在各目标独立的目录中准备锁定的 vcpkg 原生库，并生成可重定位的共享库普通文件供打包使用。在打包目录中运行 `./shinki-emu-manager`。打包会检查 ELF 架构、加载路径和依赖闭包，收集许可证及离线元数据快照；`ci` 报告的 ZIP 保留执行权限。CI 与发布任务使用同一组依赖和 xtask 命令。核心可用性仍以 `matrix` 为准，声明为 blocked 的核心不产生二进制附件。
Linux 包使用发行版提供的 glibc、libstdc++、OpenSSL 3、fontconfig、udev 和 ALSA ABI 库，商店网页登录另需 GTK 3 和 WebKitGTK 4.1。目标系统需安装对应运行包（Ubuntu 上安装 `libwebkit2gtk-4.1-0`，会一并装上 GTK 3），并提供带图形驱动的 X11 或 Wayland 桌面。
Impacto 的 Linux VA-API 库还依赖发行版的 `libdrm.so.2`（Ubuntu 包名 `libdrm2`），该库属于系统图形栈，不随核心打包。
KiriKiri2 还使用发行版的 OpenGL 分发库 `libGL.so.1`、`libGLX.so.0` 和 `libOpenGL.so.0`（Ubuntu 包名 `libgl1 libglx0 libopengl0`）。
X11 和 OpenMP 分别使用发行版的 `libX11.so.6` 与 `libgomp.so.1`（Ubuntu 包名 `libx11-6 libgomp1`）。
gdiplus 文字渲染还使用发行版的 `libpangocairo-1.0.so.0` 和 `libpangoft2-1.0.so.0`（Ubuntu 包名 `libpangocairo-1.0-0 libpangoft2-1.0-0`）。
GTK 对话框使用发行版的 `libgtk-3.so.0`（Ubuntu 24.04 包名 `libgtk-3-0t64`）。vcpkg curl 需要比 Ubuntu 24.04 更新的 OpenSSL 符号，因此必须打包匹配的固定版本 `libssl.so.3` 和 `libcrypto.so.3`，不能将它们作为系统 ABI 跳过。
Linux 仅关闭未使用的 `BUILD_EDITOR_SPINE`，保留 CocoStudio UI、MotionPlayer/Emote 和真实核心插件功能。原始 Spine 许可证随完整上游源码 tar 保留，不列入该 Linux 二进制的已使用模块许可范围；Android 和 Apple 构建不套用这一结论。
KiriKiri2 Linux 将固定 Cocos 依赖中的 FMOD 运行库打包为 `libfmod.so.6`，通过相对 `$ORIGIN` 路径加载，不将它归类为系统 ABI。项目所有者已于 2026 年 10 月 31 日授权重分发。包内 `licenses/kirikiroid2-web/FMOD.txt` 保留 Firelight 署名和准确的二进制、归档来源。Cocos 源码适用 MIT，FMOD 不改标为 MIT。源码包保留原始依赖归档、真实的 FMOD 头文件和预编译输入及重建步骤，不编造 FMOD 实现源码或历史 EULA。

RFVP 还依赖发行版提供的 GStreamer 1.x（至少 1.14）、GLib 2.x（至少 2.56）及 base、good、libav 插件，用于原生影片解码。这些库作为系统运行要求声明，不随核心复制。在 Ubuntu 上安装 `libgstreamer1.0-0 libgstreamer-plugins-base1.0-0 libglib2.0-0 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav`。包内 `build.json` 的 `system_dependencies` 记录实际依赖的系统 SONAME。

下载不稳定时，可将固定版本的源码和工具压缩包预先放入 `.work/vcpkg-downloads`，用可续传分块或 `rsync --partial` 传输。vcpkg 仍会按实际选中的版本化 port 校验每个压缩包的 SHA512。不要用发行版的 libass/FFmpeg 替换锁定的原生构建。

`deps` 默认拒绝有修改的 fork 工作区。调试未提交的 adapter 改动时，可显式传入 `--allow-dirty-sources`；工作区 HEAD 必须已是清单固定的 revision，源码准备不会对该工作区执行 fetch、checkout 或子模块更新。构建记录保留源码修改状态，`release` 拒绝有修改的源码及其构建记录。

Linux CI 与发布任务还会解压报告中的 Manager ZIP，使用独立用户数据目录在 Xvfb 下启动。进程必须保持运行 15 秒，提前退出会使任务失败。

### Android Manager 测试 APK

使用 JDK 27（设置 `JAVA_HOME`，将其 `bin` 加入 `PATH`）、Android SDK platform 37.2、build-tools 37.0.0、NDK 30.0.16248370 和 cargo-ndk 4.1.2。`ANDROID_HOME` 指向 SDK 根目录，`ANDROID_NDK_HOME` 指向其中的 `ndk/30.0.16248370`。构建只接受清单固定的稳定版 NDK revision。缺少 SDK 包时，显式调用该 SDK 的 `sdkmanager --sdk_root=<sdk> --channel=0 "platforms;android-37.2" "build-tools;37.0.0" "ndk;30.0.16248370"`。将 `CARGO_HOME`、`CARGO_TARGET_DIR`、`TEMP`/`TMP` 和 `GRADLE_USER_HOME` 放在空间充足的磁盘上。

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo xtask deps --target aarch64-linux-android
cargo xtask deps --target x86_64-linux-android
cargo xtask build --target aarch64-linux-android --manager --release
cargo xtask build --target x86_64-linux-android --manager --release
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-beta.2 --out .work/android-test
```

每次构建先读取 `cargo ndk-env` 的工具链环境，再直接调用 `Cargo --locked` 编译，静态链接 `static_manager_targets` 含该三元组的核心（RFVP、Siglus、Shinki Musica、CMVS、Artemis、Impacto、Ren’Py、KiriKiri），并在构建记录所指 target 目录的 `android-inputs/` 中收集 JNI 运行依赖、元数据和许可。打包需要**两个三元组**来自同一源码提交的 release 构建记录。Gradle 随后生成含 `arm64-v8a`、`x86_64` 和 `assets/metadata/` 的通用 APK；xtask 检查签名及每个原生库的依赖闭包。没有签名密钥时，输出为 `ShinkiEMU-<version>-android-universal-debug-signed.apk`，这是 debug 签名的 CI 测试包，不是正式安装包。测试密钥生成在 `GRADLE_USER_HOME` 中。证书验证器的 Kotlin 组件由 Gradle 从官方 Maven archive 获取，版本与 `Cargo.lock` 中的 Android shim 同步，不再从 Cargo 源码目录复制 AAR。

CMVS 通过同一静态注册表调用 `cmvs_query`，启用 `video-ffmpeg`，SDK 解码器复用 Musica 的 API 28 静态 FFmpeg vcpkg triplet 和 xtask 依赖通路。Manager 包的 `embedded-cores.json`、对应源码和许可证包含 CMVS。APK 验证会逐字节比对 `assets/licenses/shinki.emu.cmvs/` 中的许可证与清单输入。Android 实际运行按用户要求未验证。

首次下载 Gradle 依赖后，可设置 `SHINKI_EMU_GRADLE_OFFLINE=1` 传入 `--offline`；每次打包都使用新的输出目录。`cargo xtask ci --target aarch64-linux-android --component manager --report .work/android-ci.json` 构建两个 ABI，并报告实际 APK 路径。CI 先为两个三元组准备依赖，只运行一次该命令：`matrix --json --ci` 去掉了 `x86_64-linux-android` 的 Manager 行，那一行生成的是同一个通用 APK。Release 发布一个通用的 release 签名 APK，并保留对应的 sources ZIP。
八核共用现有 MainActivity。KiriKiri 的原始 SDL/Cocos Java 辅助类使用私有包名，原生闭包隔离 SDL、FFmpeg 与 JNI 符号；宿主在注册查询前初始化真实 JavaVM、Activity 和资源目录。`Tools/verify-krkr-android.py` 检查实际双 ABI APK 的 DEX/JNI、原始资源和 Spine 闭包。资源解压与打包检查保持 32 MiB 单文件上限及有界流式处理，包含 Impacto 原始 17,032,620 字节字体。debug 签名证明不能代替正式发布签名。


所有 Manager 安装包共用 `Assets/Branding/shinkiemu-icon.svg`。Rust 构建或打包时生成 Windows EXE 内嵌 ICO、macOS ICNS、iOS app icon set、Android 自适应启动器 mipmap 和 Linux hicolor PNG。Linux ZIP 还包含 `share/applications/shinkiemu.desktop`，可将它和 `share/icons/hicolor` 安装到相应的应用与图标目录。游戏替换窗口图标后，桌面运行时会在退出游戏时恢复 Manager 图标。

运行限制：原版 Slint 1.18.1 在 SwiftShader/软件 Vulkan 模拟器上存在只渲染首帧、后续界面不再更新的问题；请使用 `-gpu host` 或实体设备。独立的单 Text、单 Timer 应用在五次冷启动中均复现了更新停滞；同一 Manager 测试 APK 在主机 GPU 模式下五次冷启动均正常显示中文引导及图标。构建、打包成功不代表游戏兼容性已验证。

### Apple Manager：原生 macOS 与 Linux 交叉构建

原生 CI 继续使用 `build/targets.toml` 中的 macOS runner，通过 Cargo/Xcode 构建。在 Linux 上，`build`、`check` 自动选择 cross 0.2.5 和 `Cross.toml` 配置的私有镜像。Apple 目标包括 `x86_64-apple-darwin`、`aarch64-apple-darwin`、设备目标 `aarch64-apple-ios`，以及模拟器目标 `aarch64-apple-ios-sim`、`x86_64-apple-ios`。最低系统版本为 macOS 13.0、iOS 16.0。

`build/targets.toml` 中某个目标的 `blocked` 非空时，该目标上的所有组件都按这个原因阻塞，优先于核心的 `supported_targets` 和 `manager_supported`：任务矩阵不包含这些行，`deps`、`build`、`package` 拒绝执行，发布说明列出该目标及原因，不附产物。五个 Apple 目标目前带有这一字段，在交叉构建工具链验证通过前不进入 CI 和发布；删除该字段即可恢复。

从 [Mac App Store](https://apps.apple.com/app/xcode/id497799835) 或 [Apple Developer Downloads](https://developer.apple.com/download/all/) 取得 Xcode，遵守 Apple 许可。在 Mac 上用 `xip --expand` 展开官方 Xcode `.xip`，选择这套 Xcode，并在该 Mac 上接受许可。单独的 Command Line Tools 不提供 iOS SDK。随后执行：

```sh
for sdk in macosx iphoneos iphonesimulator; do
  tar -C "$(xcrun --sdk "$sdk" --show-sdk-path)" -cJf "$sdk.sdk.tar.xz" .
done
```

私下传输归档，在 Linux 上分别解压。`SHINKI_APPLE_SDK_DIR` 直接指向含 `SDKSettings.json`、`System/`、`usr/` 的 SDK 内容目录，不能指向父目录或归档。Darwin 使用 `macosx`，设备使用 `iphoneos`，两个模拟器使用 `iphonesimulator`。流水线不从镜像站下载 SDK。SDK 和含 SDK 的镜像不得进入 Git 或公共镜像仓库。

把源码、`CARGO_HOME`、`RUSTUP_HOME`、`CARGO_TARGET_DIR` 和工具都放在专用目录，例如 `~/shinkirou-apple/`。显式安装 Rust 1.99.1 及所选 Rust target，再通过 `cargo +1.99.1 install --locked --root "$HOME/shinkirou-apple/tools"` 安装 cross 0.2.5 和 apple-codesign 0.29.0（提供 `rcodesign`），将工具目录的 `bin` 加入 `PATH`。创建镜像要求 `/` 至少剩余 8 GiB，不会删除用户文件或镜像。在本仓库执行：

Docker 需要 Buildx/BuildKit（用 `docker buildx version` 检查）。私有插件可放在 `$DOCKER_CONFIG/cli-plugins/docker-buildx`，无需 sudo，也不改用户原有 Docker 配置。[Buildx v0.38.1 官方 Linux x86_64 附件](https://github.com/docker/buildx/releases/download/v0.38.1/buildx-v0.38.1.linux-amd64) 的 SHA256 为 `e25d5cd675092bb5d1d03632fdc46cdf3fede7b514dc5f023a105e00d3ce5ad4`，核对后再赋予执行权限。

```sh
export DOCKER_CONFIG="$HOME/shinkirou-apple/docker"
export SHINKI_APPLE_SDK_DIR="$HOME/shinkirou-apple/sdks/macosx.sdk"
cargo xtask apple-image --target aarch64-apple-darwin
cargo xtask deps --target aarch64-apple-darwin
cargo xtask build --target aarch64-apple-darwin --manager --release
cargo xtask package --target aarch64-apple-darwin --manager --version 0.1.0-beta.1 --out .work/macos-manager
export SHINKI_APPLE_SDK_DIR="$HOME/shinkirou-apple/sdks/iphonesimulator.sdk"
cargo xtask apple-image --target aarch64-apple-ios-sim
cargo xtask build --target aarch64-apple-ios-sim --manager --release
cargo xtask package --target aarch64-apple-ios-sim --manager --version 0.1.0-beta.1 --out .work/ios-simulator-manager
```

每个目标单独准备镜像并选择对应 SDK。镜像固定 Rust、osxcross 和 vcpkg 输入，提供 LLVM/LLD、C/C++、CMake 和 Apple triplet；宿主编译器与目标编译器分开配置。原生依赖许可导出到 `.work/apple-native/<target>`，镜像身份相同时才复用。缺 SDK、镜像或 Rust target 时 `doctor` 返回错误；支持目标的真实编译错误仍然按失败处理。

macOS 包含真实的 `ShinkiEMU.app`，可执行文件位于 `Contents/MacOS`，元数据和许可位于 `Contents/Resources`，`ci`/`release` 生成 ZIP。Apple bundle 也包含 ShinkiEMU 应用图标。iOS Manager 在主线程使用 Slint 的 winit/UIKit 后端和 Metal 渲染器，原生 Files 选择器保留安全作用域授权及书签，应用数据存入自身 Library 沙箱。iOS 重启需由用户关闭再打开应用。共享平台会激活真实的 AVAudioSession；Manager 编译成功不代表可以在 iOS 安装任意下载的核心二进制，核心自身的嵌入与构建限制仍在清单中。

RFVP、Siglus、Sena、Ethornell 和 ReMinori 在两种 macOS 架构上构建动态核心，并在 iOS 真机和两种模拟器架构的 Manager 中静态注册。iOS 不加载下载的核心 dylib。后继 Android Manager 静态注册 RFVP、Siglus、Shinki Musica、CMVS、Artemis、Impacto、Ren’Py 和 KiriKiri 八核，保留冻结六核的提交 pin 与资源合同，独立验收新提交的 APK、完整对应源码和 CI。构建会核对 Manager 依赖与 `build/cores.toml` 的提交 pin，安装包包含 `embedded-cores.json` 和各静态核心的许可文件。

Shinki Musica 已加入 iOS Manager 的静态注册表和核心清单，FFmpeg 依赖按目标隔离。独立提交 `72ac926e1d15a452dc81ca84f3e82776aa302a26` 的精确提交 CI，以及真机目标和两种模拟器目标的原生编译、打包、签名、依赖闭包和许可检查均已通过。后续 Manager 将此移植与原生存档合并时，仍须运行新集成提交的完整门禁。iOS 不支持单独安装动态核心；设备启动验收已获豁免。

静态链接 FFmpeg 的 Manager 会将该目标独立安装树中的 FFmpeg 及传递依赖许可一并放入 `licenses/ffmpeg/`；独立核心包复用同一收集路径。

原生静态核心通过 Cargo 构建清单记录 `native-static` 归档和运行资源。Manager 的构建记录会保留这些依赖清单，打包时将资源放入 `cores/<id>/Resources`。依赖准备按嵌入核心分别选择，`SHINKI_EMU_NATIVE_VCPKG_ROOT` 下保存各核心的安装树。Apple 静态链接会合并原生构建归档，并向 Cargo 提供目标安装树中的库及系统 framework。

原生 vcpkg 清单按 `.work/native-vcpkg/<core>/installed-<triplet>/<triplet>` 隔离安装树，准备一个 ABI 时不会删掉另一个 ABI 的依赖。构建辅助库和 xtask 共用路径映射。Android 静态链接包含核心自带的原生库、目标 vcpkg 归档及 pkg-config 的私有依赖，APK 保留 NDK 的 C++ 共享运行库。Windows 构建机上的依赖安装必须跨工作树串行执行，避免争用 vcpkg 工具目录的构建锁。

RFVP 的 Apple 字幕依赖在原生 macOS/Xcode 上按固定 vcpkg 清单构建 libass 0.17.5。每个目标单独安装静态库，triplet 分别为 `arm64-osx`、`x64-osx`、`arm64-ios`、`arm64-ios-simulator`、`x64-ios`。`build/vcpkg/apple-triplets/` 中的 overlay triplet 固定部署目标（macOS 13.0、iOS 16.0），并让 libass 用 CoreText 代替 fontconfig；`deps` 会清掉 `*_DEPLOYMENT_TARGET` 环境变量，避免 macOS 构建机工具被编成 iOS 目标。pkg-config 将私有静态依赖和 Apple framework 传给 Rust。RFVP 的 Apple WMV/MPEG 解码使用原生 Rust 实现，不依赖 GStreamer；CoreAudio 由系统提供。FFmpeg、C++ 原生核心和 CMVS 在 Apple 上的阻塞原因保持不变。

模拟器包包含 `simulator-test.app`；设备打包生成带 `Payload/*.app` 的 `device-test.ipa`，`ci` 报告该 IPA 的真实路径。打包先校验单架构 Mach-O、设备/模拟器平台以及系统/随包依赖闭包，再签署副本。Linux 使用 `rcodesign sign` 并逐个验证 Mach-O，原生 macOS 使用 `codesign`。ad-hoc 签名是真实签名，但不提供 Apple provisioning、公证或 App Store 身份。设备安装仍需有效的 Apple 签名及 provisioning，这些产物用于测试与集成，不是正式安装包。Release 名称包含 iOS 的设备/模拟器 variant，避免同架构附件重名。

SDK 验收保留真实产物路径和大小、原生架构与依赖检查，以及对应平台的正常验签输出。原生 macOS 打包使用 Apple `codesign`。缺少合法 SDK 时，这项验收仍受阻；编译成功本身不代表游戏运行兼容。

Apple Manager 的 `build.json` 记录打包改写和签名前的原生输入，因此签名后的可执行文件可以有不同的字节哈希；发行记录标识实际分发的包。保留正常的平台验签及产品、目标、版本对应检查，不把原生输入回执改标为签名后二进制的哈希。

## OpenHarmony Manager

本地构建需要 DevEco Studio 自带的 OpenHarmony Native SDK 5.0.5（API 17，HAP 面向 API 12 及以上）和 Rust 1.99.1。将 `OHOS_SDK_NATIVE` 指向 `sdk/default/openharmony/native`，xtask 会在同一 DevEco 安装中找到 Node、hvigor、ohpm、CMake/Ninja、`hap-sign-tool.jar` 和 `llvm-readelf`。把 `SHINKI_EMU_OHOS_SIGNING_DIR` 设为仓库外的目录（例如 `.shinki-secrets/ohos-debug`）：

```sh
cargo xtask deps --target aarch64-unknown-linux-ohos
cargo xtask build --target aarch64-unknown-linux-ohos --manager --release
cargo xtask package --target aarch64-unknown-linux-ohos --manager --version <version> --out <dir>
```

`deps` 用 `arm64-ohos` triplet 构建 libass 和 FFmpeg，Meson 1.12 的前缀与 Android triplet 一样改用 Windows 扩展路径。构建使用 SDK clang 链接，并打包 `libc++_shared.so`。`package` 把 Manager 库放入 `Platforms/OHOS/entry/libs/arm64-v8a`，把元数据快照、声明和许可证放入 `rawfile/bundled`，然后运行 `ohpm install` 和 `hvigorw assembleHap`。首次运行时，它用 SDK 自带的 OpenHarmony 测试 PKI 生成 debug 签名材料：在 `OpenHarmony.p12` 的副本中新建密钥，由测试应用 CA 签发应用证书，再用 profile-debug 密钥签出 `org.shinkirou.shinkiemu` 的 debug profile。签名用 `sign-app`，之后运行 `verify-app`、列出包内文件，并对每个打包的库输出 `llvm-readelf -d`。剩下的 `NEEDED` 项必须都是 OHOS 系统库。输出目录包含 `ShinkiEMU-<version>-ohos-arm64-v8a-debug-test-signed.hap`、`ohos.json` 和 `build.json`。不要把 keystore、证书或 profile 提交到 Git。

测试 PKI 签名的 HAP 按实验性测试包发布；签名、包内容和原生库依赖闭包会核验。设备/模拟器启动验收已获豁免，未验证零售设备可安装性；要核验可安装性，需要 DevEco/AppGallery Connect 签发的 profile。

## 矩阵与 CI

`matrix` 列出全部已声明的 Manager/核心组合及阻塞原因。`matrix --json` 只安排有 GitHub runner 的任务；OHOS Manager 标记为支持实验性测试签名交付，但 `runner = "local-only"`，其 DevEco 构建在本地交付。`--ci` 还会去掉重复的 Android x86_64 Manager 行。当前组合见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)。

```sh
cargo xtask ci --target x86_64-pc-windows-msvc --component rfvp --report .work/reports/rfvp-windows.json
```

未支持的组合报告 `blocked` 和 `artifact: null`。OHOS Manager 本地签名构建必须与同一提交的成功远端 CI 汇合后才能发布；受支持组合编译失败仍报告 `failed`。`publish_blocked` 单独记录分发限制。

GitHub Actions 是权威 CI，构建有托管 runner 的发布产物。仅 DevEco 可构建的 OHOS HAP 由下述有门禁的本地流程提交。
发布含 OHOS Manager 的版本时，先推送 release 分支，等待同一提交的 `ci.yml` 成功。以该分支和版本触发 `release.yml`，传入 `components=manager`；工作流核对全部托管平台构建行，不会自动发布 Manager。把此运行的每份 `release-files-manager@*` artifact 下载到临时目录，并将其中的 `<tag>/<target>/` 合并到干净且处于同一提交的 `.work/release/<tag>/`。设置 DevEco SDK 与仓库外签名目录后执行：

```sh
cargo xtask deps --target aarch64-unknown-linux-ohos
cargo xtask build --target aarch64-unknown-linux-ohos --manager --release
cargo xtask release --component manager --target aarch64-unknown-linux-ohos --version <版本>
SHINKI_EMU_RELEASE_CI_RUN_ID=<成功的-ci-运行编号> SHINKI_EMU_RELEASE_HANDOFF_RUN_ID=<成功的-handoff-运行编号> GH_TOKEN=<GitHub-token> cargo xtask release --component manager --version <版本> --publish
```

执行最后的发布命令前，在同一提交创建 `manager-v<版本>` 草稿发布，上传 OHOS HAP、对应源码 ZIP，以及重命名为 `ohos-handoff.json` 的 OHOS `release.json`。以同一提交触发 `release.yml`，传入 `components=manager`、版本和 `verify_local_manager=true`。此运行只验证、不重建或发布：下载草稿中的实际产物，核对 SHA256、HAP 内容、原生依赖闭包与对应源码提交，无需托管 runner 安装 DevEco。已核验文件保留为 `verified-ohos-handoff` artifact。将成功运行编号设为 `SHINKI_EMU_RELEASE_HANDOFF_RUN_ID`。测试 PKI 签名由本地 DevEco 打包核验；托管交接门禁核验产物完整性，不将其描述为再次验签或设备验收。

发布命令核对所有产物的提交、归档 SHA256 和全部托管平台产物（Android x86_64 重复行除外），要求 OHOS HAP，并向 GitHub 核对同一提交的两项工作流门禁。发布名称和说明写明测试签名与启动验收豁免。

源码包的字幕库重建命令包含 `build/vcpkg/mobile-triplets`，其中提供 OHOS 的 `arm64-ohos` triplet；对应原生源码归档随包提供。
仅验证的交接任务需要仓库 `contents: write` 权限才能读取未发布的草稿附件，任务步骤不会发布或修改草稿。

本地 runner 只是推送前的可选预检，用来提前发现会让远端失败的问题。`matrix --json [--ci] --since <ref>` 只保留输入在 `<ref>` 之后有变化的行，runner 通过 `SHINKI_CI_SINCE` 传入该参数。共享输入会选中所有行，包括根目录 `Cargo.toml`/`Cargo.lock`、`rust-toolchain.toml`、`xtask/`、`Source/{SDK,FamilyApi,Tools}/`、`build/targets.toml`、`build/vcpkg/`、`build/ci/`，以及这里没有列出的任何路径。`Source/Manager/` 和 `Source/Providers/` 选中 Manager 行。`Source/Families/<目录>/` 选中 manifest 位于该目录的核心，以及该核心 `static_manager_targets` 上的 Manager 行；`build/cores.toml` 中某个 `[[cores]]` 表有改动时，对该核心的效果相同。`docs/**`、`Metadata/**` 和 Markdown 文件不选中任何行。

## 打包与发布

先构建所选组件，再打包构建记录指向的产物：

```sh
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --out .work/packages/rfvp
cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-beta.1 --out .work/packages/manager
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc
```

输出目录必须尚不存在。打包拒绝不安全路径、链接、冲突、缺许可、架构错误、缺 query 导出和原生依赖不完整的输入。核心包包含库、资源、依赖、许可证、构建来源及带哈希的 `core.json`；源码包保留适用的对应源码与重链接输入。

Manager 和各独立核心包都会按所选 manifest、package、features 和目标平台收集自己的 Cargo 依赖闭包（普通依赖和构建依赖，不含仅供测试的依赖边）。各 crate 声明的 `license_file`、源码根目录中的 `LICENSE*`、`COPYING*`、`COPYRIGHT*`、`NOTICE*` 文件及同类许可目录的内容，放在 `licenses/rust/<名称>-<版本>/`，同目录的 `package.json` 记录 SPDX 表达式和文本来源。成员 crate 没有许可文件时，也会查找 Git checkout 或 workspace 根目录的原始声明。仍无文件时，打包器从固定的 `spdx` 0.13.6 内置离线数据库中，取得 SPDX License List 3.29.0 的标准文本，将表达式引用的全部许可和例外条款写为 `LICENSE-<SPDX>`。来源版本与压缩包 SHA256 记录在 `build/dependencies.toml`，并与 Cargo 固定依赖核对；打包时不下载许可文本。无法识别的回补标识符，或既无许可文件也无表达式的 crate，都会使打包失败。Material 组件、图标及原生依赖的许可单独收集。

`fonts/` 中的字体许可和署名文本按原名保留。`LicenseRef-UFL-1.0` 这类自定义条款必须有对应的上游文本，例如 `fonts/UFL.txt`，打包器不会编造许可正文。crate 只提供字体声明时，代码本身声明的标准许可仍会补齐。

`shinki-emu-<slug>-<版本>-<平台>-sources.zip` 包含 `source.tar` 和 `BUILD-FROM-SOURCE.md`。TAR 中是发布提交时产品的已提交源码树，以及所选 fork 在固定提交上的已提交源码树，两者的子模块都按记录的提交展开。工作区里未提交的修改不会进入源码包。Git LFS 指针会先按提交记录核对 SHA256 和大小，再替换为已展开的真实数据；数据缺失时先运行 `git lfs checkout`。

Rust 依赖以未改动的 `Cargo.lock` 为准（registry 校验和与准确的 Git 提交），重新构建时由 Cargo 下载。`rust-sources/<名称>-<版本>+<来源>/` 另外保存许可要求随附源码的已链接 crate：所有可选许可都是 copyleft 的 crate（例如 MPL-2.0 的 symphonia、triple_buffer、option-ext）、没有 SPDX 表达式的 crate；只要有一个已链接 crate 只能按 GPL/LGPL/AGPL 使用（ReMinori），就收录全部已链接 crate。带宽松许可选项的 crate（`Apache-2.0 OR GPL-2.0-only`，以及 Slint 的 `GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR …`）按该宽松选项分发，不产生源码义务。路径 crate 已在 fork 快照中。

原生源码覆盖所选安装树中的全部 vcpkg port，依据 `vcpkg/status` 跟随依赖，包括构建机 triplet 的工具。`native/ports/<分组>/` 保存每个 registry port 的配方和补丁，从安装时 SPDX `gitoid` 记录的固定 registry tree 中提取；overlay port 没有 tree 标识，随 fork 快照交付。`.work/vcpkg-downloads/` 保存已安装 SPDX 按 SHA512 记录的源码压缩包。构建说明会克隆固定版本的 vcpkg。Impacto 另外保留准备好的 LibAtrac9/magic_enum 源码，mspack 随 fork 快照交付。

源码包直接复制 `deps` 和 `build` 已核对过的输入：不再重新计算 `.work/downloads` 的哈希、不再校验配方、不做 Cargo 离线解析，也不重复检查工作区是否干净。`release` 本身拒绝未提交修改的产品检出，以及来自其他源码提交的构建记录。

Ren'Py 需要 `.work/renpy-build` 位于 `7bfab40c1174f622f644b24669afd5fb167fbb79`，并保留其中的 FFmpeg/FriBidi/SDL 源码和补丁。fork 的 `licenses/renpy/{native,python}-sources.json` 记录额外原生源码、SDK 实际复制的 Python 包版本、URL 和哈希；`deps` 把这些文件下载到 `.work/downloads/` 并核对哈希。源码包包含这些压缩包、可读的 chardet/certifi 源码、基于压缩包的组合运行时构建入口，以及重链接、替换和调试权利说明。SDK 的 urllib3/certifi 版本与标签配方不同，构建说明会列出已核实的 SDK 版本，不声称仅凭该标签即可逐字节复现。Ubuntu 交叉构建工具及 Windows 编译器/系统 SDK 属于构建前提。

没有 `url` 的记录改为固定 `repo` 和 `rev`，目前是托管在 Gitiles 的 aom 和 libyuv：Gitiles 每次生成的压缩包字节都不同。`deps` 会浅取该提交、核对取到的提交 ID，再对 `git archive --format=tar` 生成的未压缩 tar 计算哈希；不同 Git 版本生成的结果一致。

`cargo xtask deps --core renpy --target x86_64-pc-windows-msvc` 会准备固定版本的 `.work/renpy-build` 检出和递归子模块。已有工作树包含未提交修改时，命令会在更新前失败；源码收集过程保持只读。

Sena 按已批准的上游分发策略保留 MS PGothic 字体。包内 `licenses/sena-rs/MS-PGothic.txt` 记录上游没有提供明确再分发授权，核心的 MPL 许可不替代字体本身的条款。

源码许可收集也覆盖嵌套资源的许可文件和字体同名文本说明，保留相对路径，不复制字体本身。Git 工作区成员的包目录没有许可文件时，会收集该检出根目录提供的许可文件，先保留上游版权声明，再考虑使用 SPDX 标准文本。

Manager 与自研 CMVS/Musica 保留各自的 SemVer 和 `manager-v<semver>`、`core-<slug>-v<semver>` 标签，仍需 `--version`。RFVP、Artemis 等第三方核心从 `build/cores.toml` 计算版本，不传 `--version`。`cargo xtask release --component <id> --dry-run` 只核验上游 ref、完整提交与适配分支 pin，不构建或发布；`release --target <三元组>` 在 `.work/release/<tag>/<目标>/` 写入二进制 ZIP、源码 ZIP 和 `release.json`。被阻止的目标及 `publish_blocked` 核心不产生文件。

Manager [0.1.0-beta.7](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.7) 已从 `fd9c78a4cabf1d80430744e4bb5d140c2a837bb9` 公开全部九个目标。同提交的 CI、托管准备和 OHOS 交接均成功，每份安装包及对应源码都属于该产品提交。Android 内置七核，使用既有正式发行证书 `c415bd462d26574758ed455055ff528a3bd6036e39d5ac6887308f8cf69e97fc`。iOS 保持 RFVP、Siglus、Shinki Musica、Sena、Ethornell、ReMinori 六核，设备和模拟器包仍为测试产物；OHOS 保持 SDK debug-test-PKI 测试签名边界。来自 `fef31a82104a371aa919641f1fe08c65285e5267` 的 beta.6 及更早公开 tag、资产不改写。后续仅源码中的功能和核心 pin 更新不属于 beta.7 下载包。

[KiriKiri2 序号 2 发行版](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-kirikiroid2-web-branch-web-1e3603089b4951c8e2831d978ea6fe5a8097bceeb0a56fdeb65aa3953e67eecf) 已从产品 `fe4e6f2d82c8b61f3f14e13d48af6d57722b9952`、适配提交 `d72046c28e6ada623acdb1d72f7075edd488797d` 公开 Windows、Linux 和两种 macOS 架构。相同适配已静态嵌入 beta.7 Android，不代表 KiriKiri 已进入 iOS 或 OHOS。桌面包附对应源码和原许可证，FMOD 条款单独保留。

第三方身份分别保留原作者的 `upstream {repo, ref_kind, ref_name, commit}` 与组织适配仓库的 `adapter {repo, branch, commit}`，提交均为 40 位小写完整哈希。上游提交有已核验标签时用标签，否则用真实上游分支；不能把组织集成分支当上游。显示为 `<ref>@<上游提交前12位> (adapter <适配提交前12位>)`。文件版本是 `<tag|branch>-<ref slug>-<digest>`：取 ref 的前 16 个 ASCII 字节，字母转小写、非字母数字逐个换成 `-`；digest 为八个 UTF-8 字段的完整 SHA256，顺序为上游仓库、ref 类型、ref 名、上游提交、适配仓库、适配分支、适配提交、十进制 `release_sequence`，每项后接 LF。版本最长 88 字节，标签为 `core-<plugin slug>-<version>`，plugin slug 把 ID 转小写并将点和下划线换成 `-`。分支中的斜杠不会进入路径；仅适配代码更新也会生成新身份。

第三方 plugin slug 最长 32 字节，因此 Release 标签最多 126 个安全 ASCII 字节。

`release_sequence` 是每个第三方核心独立、严格递增的正整数发布序号，上限为 9007199254740991；换上游、换适配 pin 或主动重构建时递增。标签版与分支快照共同按此序号选最新，不比较分支名或哈希大小。新身份优先于旧 beta 记录，其余版本按 SemVer 排序；不同版本不能复用序号，跨平台也一样。核心包、嵌入核心清单、源码引用和目录携带一致的来源身份；上游版权许可与完整对应源码要求不变。

[Impacto 序号 3 发行版](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-impacto-tag-0-10-8-930-96de7434c8b1a57df7ff24dcee829282194873fe596677056fa168485cb1555e) 已公开 Windows、Linux 和两种 macOS 架构的包，产品提交为 `5b197c1c656186d3a535bb3486311623516166e5`，上游为 `0.10.8.930`，适配提交为 `34b89c29123d025d96f73ac9a746fb388e572065`。四个平台的对应源码、原生输入、许可和包审计均通过，八个公开二进制/源码附件也已无认证实际下载并核对哈希。此独立发行不替换 Manager6 内置的 `910a820ba3ea8d4dd8d6e19c560df1d5a3bc5157`/序号 2，也不改写已公开的序号 1 资产；目录按序号选中 3，不按发布完成先后排序。

[Ren’Py 序号 1 发行版](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-renpy-tag-8-5-3-26051504-5f537560e1a1b852070dec65946d6c027e9f4f7482bddd8349ed47960c383133) 已公开四个桌面平台，产品提交为 `05326439dcfafb81df79f3dabe0997fb696ad3f3`，上游为 `8.5.3.26051504`，适配提交为 `a52d9d9c006223c2b5a276c05e1b97ffeecbe66f`。每份真实对应源码包的固定重构建配方均逐项核验通过 389/389，包括 107 字节的符号链接目标。八个公开二进制/源码附件已无认证实际下载，哈希均与已验收记录一致；原生许可正文也已逐字节核对固定 fork 和原始输入。

同一索引额外发布 `cores-v2.json`（`shinki.emu.cores.v2`），包含旧记录和新身份。`cores.json` 保留 v1 以及所有无 identity 的已发布记录，旧 Manager 仍能下载；新 Manager 优先读索引中的 v2，也接受旧 v1 快照。已有 beta 的标签、附件与来历均不改写，仍可安装。安装更新比较包身份，不比较适配 crate 自身的版本号。

上游身份核心包至少需要 Manager `0.1.0-beta.6`。beta.5 及更早版本会拒绝新增的包身份和更长的版本字段，必须保留它们可用的旧目录与旧核心包。每次新的第三方核心发布都要注明此要求。兼容 Manager beta.6 与新版 RFVP 均已可公开获取，Sakura 配方 0.2 现已开放；转换仍须使用新的输出目录，原游戏和存档不改。

`cargo xtask release --component <id> --version <semver> --publish`（需要 `GH_TOKEN` 或 `GITHUB_TOKEN`）收集该 tag 下所有已准备的目标。它拒绝来自其他产品提交或核心固定版本的记录、已指向其他提交的 tag、从其他提交创建的 draft，以及空 Release 和已发布的 Release。它在本次运行的提交上创建 draft，或复用该提交上的 draft（替换中断运行遗留的同名附件），上传源码包和二进制包后发布；tag 不存在时由 GitHub 在该提交上创建。核心发布之后，它把每个平台的目录条目提交到 `metadata/release-<tag>` 分支的 `Metadata/engines/cores/` 下，再向 `main` 提 PR。组织设置不允许 Actions 创建 PR 时，最后这一步会失败，但所有条目已在分支上，手动提 PR 即可。目录 URL 用最终 tag 构造，不使用 draft 的 `untagged-*` URL。Manager 与核心独立发布；Android debug 签名测试包记为阻止，不会上传。

`release.yml` 构建所选组件在托管平台的所有行，并自动发布不含本地专用目标的组件。含受支持 OHOS 目标的 Manager 由上文有门禁的本地流程补齐 HAP 与源码后发布。单个核心可推送 tag；多个组件用 `version` 和 `components` 手动触发工作流。一次推送超过三个新 tag 不会触发工作流。

需要只准备、不公开时，仍运行这份 `release.yml`，传入 `prepare_only=true`，并可用 `source_commit` 指定精确的 40 位产品提交。显式指定源码提交只允许用于仅准备模式。原生构建、打包、平台签名及源码和许可证打包命令保持不变；最终交接要求每份记录对应实际产品提交、组件、版本和目标，并跳过公开发行步骤。workflow dispatch SHA 只标识编排代码，不是产品来源。从已接受产品提交的干净检出用现有 xtask 发布实际产物；CI 成功不代替产物可获取、正常包结构及平台签名要求。

工作流的 `version` 只适用于 Manager/自研核心；第三方组件在展开平台矩阵前分别核验并计算自己的版本。第三方执行上面的发布命令时省略 `--version`，Manager/自研核心仍须传入。

元数据命令见 [Metadata](metadata.md)，核心包契约见[核心 ABI](core-abi.md)。

