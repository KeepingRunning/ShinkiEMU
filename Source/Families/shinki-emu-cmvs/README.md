# CMVS

ShinkiEMU 的 CMVS 核心，以 cdylib 导出 `shinki_emu_query`，静态注册用 `cmvs_query`。

## 支持的行为

- 游戏识别：检查游戏根目录和 `data/pack` 下的 CPZ5 标头，不依赖作品名。
- 配置：`cmvs.profile.json`（`shinki.emu.cmvs.profile.v2`），有序 `archives` 和 `loose_files`；入口脚本默认 `start.ps3`，从 `cmvs.cfg` 的 `SCRIPT_INIT_PATH` 建立 slot 0。
- 资源：只读挂载 CPZ5，PB3 图像解码，脚本、纹理和八个原生搜索域按槽号与归档索引顺序查找。
- 脚本：PS2A VM，原生调度器、返回码弹栈、0–3 号 script frame、128 个 Shift-JIS 全局字符串，以 60 Hz 逻辑 tick 运行。
- 渲染：通过 RoukakuEngine wgpu 离屏渲染器合成场景，包括纹理父子槽、滤镜链、效果通道和 quad、交叉淡化、投影网格、像素遮罩、粒子与场景 kind 50–57、镜头过渡和屏幕震动。
- 文字：12 个原生文字槽，包含禁则、ruby、描边、渐变和自定义字形。所有平台用同一个纯 Rust 字形光栅器按原引擎 `GetGlyphOutlineA(GGO_GRAY4_BITMAP)` 的规则生成字形：字体自带 TrueType 指令在像素字号上对齐网格，4×4 子像素采样得到 17 级覆盖率；墨迹外框、等宽 DBCS 双倍字宽和 600 字重的合成加粗按 GDI 规则处理，字宽与 GDI 一致。灰度位图与 GDI 并不逐像素相同：以 MS Gothic 12–48 px 全部 7494 个 CP932 字符对比，着墨像素平均相差 0.05–0.43 级，外框位置和尺寸一致的字形占 64–88%。字体从系统字体（Android 另扫 `/system/fonts`）按族名匹配；未安装时与 GDI 字体映射一样替换，替换为核心内置的 Noto Sans JP（OFL）。
- 音频：交替 BGM、交叉淡化、淡出、语音和音效，支持 MV2、MP3、PCM WAV 解码和变速。MP3 用 symphonia 解码，并复现原引擎经 ACM MPEG Layer-3 编解码器的可见行为：`acmStreamSize` 长度、8192 字节输入缓冲、每次最多 32 KiB 输出、帧需看到下一帧头才解码，以及不发送 END 时末尾缓冲帧不输出。混音输出为 48 kHz 立体声，经 SDK `NativeAudioOutput` 播放；设备不可用时打开或播放失败。
- 视频：CMV 与 MGV 播放；启用 `video-ffmpeg` 特性后解码视频。
- 存档：原生 cmd693/694 存读档、系统存档、存档目录和缩略信息；损坏的候选存档不会覆盖当前状态。
- 输入与窗口：确认键（Enter、Space、小键盘 Enter、主指针按钮）的按下和释放锁存，以及 skip/auto 模式；窗口标题和 ICO 图标经 window service 设置。
- 设置：按原生 `CONFIG_PUT/REG` 读取 INI（CP932 或 UTF-16LE）；原引擎写在 `HKCU\Software\<REG>`（32 位视图）的值改存游戏目录 `.shinki/cmvs/registry.json`，键名、值名与 DWORD/字符串类型保持原样，键不存在时回落到 INI，与原引擎一致。核心不写回。
- 会话：VM、场景和媒体在 owner 线程上运行；取消会停止 VM 和音频，关闭时依次回收所有 owner。同一进程同时只允许一个会话。

## 构建

```sh
cargo check -p shinki-emu-cmvs --all-targets --locked
cargo test -p shinki-emu-cmvs --locked
```

核心不依赖 Win32：Windows、Linux、macOS 与 Android 使用同一套文字、MP3 和设置实现。存档仍在原引擎的 `game/save/` 位置和格式。

Android Manager 在 ARM64 与 x86_64 上通过同一静态注册表调用 `cmvs_query`，启用 `video-ffmpeg`。视频解码复用 SDK 的静态 FFmpeg 和 Musica 已用的 API 28 vcpkg triplet、xtask 依赖准备及 APK 打包流程；不提供单独安装的 Android 动态核心。APK 的 `embedded-cores.json` 包含 CMVS，许可证和对应源码随 Manager 打包。Android 实际运行按用户要求未验证。

创建场景的测试需要硬件 GPU。游戏原始文件、密钥和解码产物不进仓库。
