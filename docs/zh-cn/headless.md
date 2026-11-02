---
title: 无窗口运行
description: 核心出帧验收、输入回放、截图基线比较和 Manager 界面截图。
---

# 无窗口运行与界面截图

`shinki-emu-headless` 复用 Manager 的 C ABI v3 loader、原生会话和 CPU 帧租约。输入脚本直接使用已 pin 的 RoukakuEngine `roukaku-headless-protocol` 中的 `roukaku.user_input_sequence.v1`，没有新增第二套输入协议。完整字段说明和脚本样例见[英文接口文档](../en/headless.md)。

## 构建与核心验收

```powershell
cargo build -p shinki-emu-headless
cargo test -p shinki-emu-headless
cargo clippy -p shinki-emu-headless --all-targets -- -D warnings
shinki-emu-headless run --core core-package.zip --game-dir C:/Games/MyGame --frames 180 --capture-frame 1,60,180 --output artifacts/run
shinki-emu-headless run --core core-package.zip --game-dir C:/Games/MyGame --frames 180 --script replay.json --output artifacts/compare --baseline artifacts/run
shinki-emu-headless compare --actual artifacts/compare/frame-000180.png --baseline artifacts/run/frame-000180.png --max-channel-delta 4 --changed-pixel-ratio 0.001
```

`--core` 接受 ZIP、解压目录、动态库或宿主静态注册的核心 ID。`--no-default-features` 可构建不带 Manager UI 的核心运行器；`--features static-cmvs` 提供静态 `shinki.emu.cmvs`，`--features static-siglus` 提供与 Manager 同 pin 的静态 `siglus_rs`。Manager 对应命令为 `shinki-emu-manager headless ...`，与图形界面共用 `static_cores::STATIC_CORES` 清单。

用 `cargo build -p shinki-emu-manager --features static-cmvs` 构建 Manager，才能在其 GUI owner 和 `shinki-emu-manager headless --core shinki.emu.cmvs ...` 中注册 CMVS。单独启用独立运行器的 feature 不会替 Manager 注册核心。桌面版本未启用该可选 feature 时，仍使用原有的动态核心发现。

运行器先把整个游戏复制到独立临时目录，再把副本路径交给核心。游戏目录内的存档与配置写入只影响副本；不把原存档目录作为可写路径传给核心。大游戏需要足够的临时空间。符号链接和特殊文件会被拒绝。核心是本机可执行代码，这不是操作系统沙箱；配置中明确指向游戏目录外的路径应改为临时路径。

核心在单独子进程中启动，HOME、AppData、XDG、缓存与 TEMP 指向临时位置，原生线程启动后不再修改进程环境。核心声明字符串 `save_root` 时，运行器始终把它绑定到独立临时存档目录，覆盖传入值。输出目录不能位于源游戏内，也不能与基线目录相同。

owner 线程与 Rust 原生工作线程的默认栈预算为 8 MiB；核心调试构建若需要更大的启动临时空间，可显式设置更大的 `RUST_MIN_STACK`，运行器会保留它。核心自行指定的线程栈大小仍由核心负责。

核心的尺寸、位置、模式、可见性与最小化请求作用于一致的虚拟显示状态，不创建系统窗口；状态查询返回该虚拟状态。需要用户回答的原生模态请求明确失败，宿主不会编造对话框答案。

`--configuration` 使用 Manager 原有 JSON 数组，例如 `[{"id":"profile_path","value":{"String":"key.toml"}}]`，字段 ID 以核心声明为准。`--startup-timeout-ms` 默认为 30000，从原生 open 返回后开始等待首个完整 CPU 帧。不会关闭渲染、脚本或视频来通过验收；核心自身需要的 GPU 和音频设备仍需可用。

## 时间、回放与基线

C ABI v3 由核心持有游戏时钟，没有确定性单步接口。`--frames` 指首帧之后 60 Hz 的宿主呈现采样数，不代表精确推进相同数量的原生 VM 帧。`--duration-ms` 指最短时间线长度；两者取较大值。报告中的 `acquired_frames` 是实际取得的不同完整帧数量，静态标题画面可以在多个采样点重复。精确重放取决于核心本身的时序与状态。

`--capture-frame 1,60,180` 按采样号截图；`--capture-ms 2500` 按时间点截图并向上取整。末帧始终导出。`--baseline <目录>` 比较同名 PNG；`--baseline-hash sha256:<64位十六进制>` 断言末帧的规范像素 hash。

脚本是 `InputMessage` JSON 数组，每项包括 `schema`、固定 `session`、递增 `sequence`、不下降的 `tick` 和 `event`。支持 `resume`、`focus`、`keyboard`、`ime_commit`、`pointer_move`、`pointer_button`、`wheel`、`advance_ticks`、`await`、`checkpoint`。点击由移动、按下和释放组成；指针坐标是归一化的 0..65535。按键使用 `Enter`、`KeyA`、`ControlLeft` 等物理名称，按下与释放分别记录。

归一化指针先映射到当前显示帧的光栅像素，再由核心转换为逻辑坐标；逻辑尺寸与光栅尺寸不同时，运行器不重复缩放。

`checkpoint` 的 `id` 同时用作截图名和基线文件名，不能重复或使用保留的 `frame-` 前缀。`await` 支持观察 `frame` 是否存在，以及 `frame.sha256` 是否等于 `value_hash`，在 `timeout_ticks` 内未满足即断言失败。`continue_at_match` 必须为 false；原生时钟无法回退。等待期间仍按计划截图。时间线要留够加载与断言的时间。

## 输出约定

核心帧和 Manager 界面截图共用 `<id>.png` 与 `<id>.json`。sidecar 包含尺寸、采样号、像素 hash、PNG 文件 hash 和比较结果。核心成功运行另写 `run.json`；命令失败写 `failure.json`。基线不匹配时保留实际 PNG 和比较 sidecar。

像素统一为从上到下、紧密排列的 RGBA8 sRGB，alpha=255；先处理 BGRA、行间距、倒置行和线性 sRGB，再计算带 `sha256:` 前缀的 SHA-256。PNG 编码差异不会影响像素 hash。默认精确比较；启用容差时，任一通道差值大于 `--max-channel-delta` 的像素计为变化，变化比例不得超过 `--changed-pixel-ratio`。尺寸必须一致。

退出码为 0 成功、1 输入/文件/ABI/运行/无帧失败、2 独立程序的命令行语法错误、3 基线或脚本断言失败。Manager 保留已有的语法错误退出码行为。

## 截取 Manager 自身界面

```powershell
shinki-emu-headless ui --page library --language zh-CN --theme dark --width 1600 --height 1000 --output artifacts/ui/dark
shinki-emu-headless ui --page cores --language en --theme light --output artifacts/ui/light
shinki-emu-headless ui --page patches --language ja --theme dark --output artifacts/ui/dark
shinki-emu-headless ui --page settings --settings-category library-roots --language zh-CN --output artifacts/ui/dark
```

页面为 `library`、`cores`、`patches`、`settings`；语言为 `zh-CN`（也接受 `zh-cn`）、`en`、`ja`；主题为 `dark` 或 `light`。宽高范围为 360..8192，DPR 固定为 1。Manager 命令为 `headless-ui`。

实际 `ManagerWindow` 与 `SlintManagerAdapter` 通过已有 OHOS 使用的 `MinimalSoftwareWindow` 软件渲染路径离屏输出，不创建系统窗口，也不启动真实库扫描、账号、商店或网络服务。默认数据是自制演示文字，无商业游戏封面或画面。可用 `--demo-data` 提供 `{"games":[{"title":"自制示例","description":"界面演示"}],"roots":["C:/ShinkiEMU/Demo"]}`，这些字符串只用于显示，不会扫描目录。

文件名为 `<page>-<language>.png`，附 hash sidecar 和记录数据来源的 `.source.json`；深浅主题使用不同输出目录。设置分类可选 `appearance`、`library-roots`、`network`、`translation`、`input`、`about`。
