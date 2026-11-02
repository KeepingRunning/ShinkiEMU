# Musica core

Musica 通过 C ABI v3 的 `shinki_emu_query` 提供原生核心；静态集成使用同一 `musica_query` 函数表。VM 在核心 owner 线程推进，输入进入队列，状态与帧查询只读快照。核心自己打开 PAZ、执行 SC VM、混音并维护存档。[shinki-emu-sdk](../../SDK/shinki-emu-sdk/README.md) 复用 `roukaku-text` 提供真实字体 shaping/排版/字形资源管理，并管理有界纹理缓存。场景和文字直接由 RoukakuEngine 的 `WgpuOffscreenRenderer` 在硬件 GPU 上合成，再回读最终帧交给 C ABI；不创建 Engine World、package、registry、PlatformHost 或 Headless server。GPU 初始化失败明确报错，不退回 CPU 渲染。

动态插件通过共享诊断桥将核心 tracing/log 接入 Manager；静态核心使用宿主订阅器。日志不另建文件 sink，正文和未审查 Debug 内容不会跨 ABI 透传。

## 平台依赖

`cargo xtask deps --target <triple> --core shinki.emu.musica` 使用同一份
`build/vcpkg/ffmpeg/vcpkg.json`，固定 FFmpeg 7.1.2，保留完整内置解码器、demuxer
和 parser。Windows 沿用 `x64-windows` 动态闭包，Linux 沿用 `x64-linux-dynamic`；
Apple、Android 和 OHOS 使用静态库，不另打包 FFmpeg dylib/so。每个非 Windows
target 独立安装到 `.work/vcpkg-ffmpeg/<triple>/installed/<triplet>`，避免 manifest
安装裁掉其他设备或模拟器的依赖。

Android Manager 通过 `musica_query` 静态注册核心，并启用 `ffmpeg-vcpkg`。iOS 静态 Manager 集成仍待 AppleNative 完成远端 macOS 构建与验证。Apple 构建使用原生 macOS/Xcode，部署版本为 macOS 13、iOS 16。Android FFmpeg 使用 API 28 静态 triplet。OHOS 依赖准备读取 `OHOS_SDK_NATIVE`；Windows 默认指向已安装的 DevEco Studio native SDK，并使用 API 17 arm64 工具链。xtask 为这些平台隔离 pkg-config，启用完整静态依赖闭包，并向 native 消费者导出 `SHINKI_EMU_FFMPEG_INSTALLED`；Windows/Linux 不设置此变量。

私有 `key.toml` 仍由用户显式选择，游戏内相对路径按游戏根解析，不依赖进程当前目录。
手机端需把游戏和密钥放入 Manager 可读的本地目录，并保留游戏目录内
`.shinki/musica` 的写权限；核心不扫描系统密钥目录，也不复制密钥到应用包。


## 启动和输入

原生 C ABI v3 descriptor 提供 `profile_file`（界面显示 key.toml，默认 `key.toml`）、`entry_script`（默认 `test.sc`）等 typed 启动配置，完整字段以 Rust descriptor/schema 为准。正常启动默认 `launch_mode=title`，先显示原生标题菜单，点击开始后才执行入口脚本；`direct` 保留为显式直入与原版 eden 存档导入用途。已保存的显式选择不自动覆盖。key.toml 可选显式绝对路径，或游戏目录内安全相对路径；入口是 scr archive 内单个 `.sc` 文件名。不会在入口失败后猜测另一个脚本。

已接入的渲染路径保留既有 1280×720 坐标与 Noto Sans JP 字体绑定。Host resize 只改变窗口，不改游戏内部布局。Session 按 60 Hz 逻辑处理有界 elapsed interval，显示纹理、消息、面板和已验证的 CrossFade2；字形结果与纹理有界缓存。Enter、Space 和鼠标主键推进消息；F5 保存，F9 读取。关闭和销毁都会取消文字请求、取消设备队列写入并等待 VM、audio 和 decoder owner 结束；同进程不能同时打开两个 Musica session。

正文替换是可选服务；禁用时直接显示原文。启用时只暂停当前消息推进，效果和音频继续。请求具有 session/generation identity，加载和关闭取消旧请求。失败、15 秒 timeout、错误 response 或无法排版的替换保留原文，不自动重试。正文和错误 payload 不写日志。

本地化补丁由 Manager 离线写入原生 SC，核心不再读取 `translation_table` 或 `translation_mode`，也没有补丁 overlay/Tab 切换分支。转换复用原生 PAZ/SC 解码器，只改消息姓名、正文和选项的显示操作数，分支目标、语音资源和原脚本控制保持不变。旧字典的有序 first-record/131 rolling hash 仅在转换时使用，不会随游戏安装运行时翻译表。

原生 SC 的显示操作数支持 `\u{hex}` Unicode scalar 字面转义：译文中的空格、冒号、反斜杠和 `\a`、`\v` 等字符不会变成分词、选择目标或 VM 控制。新格式要求真实 descriptor 声明 `CAP_NATIVE_EMBEDDED_TEXT`（256），Manager 在导入和启动前检查。译文直接进入 VM 消息、历史和原生存档；恢复不把已经显示的文本重新解析为脚本。`font_path` 为空时沿用打包字体；配置时读取实际字体的首个 face、内部 family 和 Unicode coverage，交给同一个 Cosmic provider，缺失、损坏或缺字明确失败。

`scr_archive`、`sys_archive` 可给出游戏目录内物理 PAZ 别名，密钥 role 与 `musica:/scr/`、`musica:/sys/` URI 不变。`script_directory` 选择同目录内标准 loose SC，脚本与 include 均先 loose、再当前输出根的 PAZ；既有逐脚本编码探测保留未替换的 Title。PNG 替换继续使用 key.toml 的 `texture_overrides`。这些路径不需要 Lua parser 或运行时解包器。

原生 `@stage` 使用与 `.stage` 相同的舞台语义，保留原始源字节；其他未知 `@` 命令不据此推断。选择对允许一个末尾分隔空格，显示与分支目标共用相同解析规则。背景层按原生 Opaque 复制 RGB，即使素材 A=0 也不改写像素；前景继续沿原混合语义。

影片字幕由 `ffmpeg-vcpkg` 启用真实 SDK/共享 libass 消费器。`subtitle_map` 为空时不建立轨道；非空必须指向游戏目录内 `{"version":1,"tracks":[{"clock":"movie","resource":"op.avi","ass":"subtitles/op.ass","fonts":["fonts/actual.ttf"]}]}`。只接受已有原生 movie 时钟，BGM 映射明确拒绝。ASS 为 UTF-8（可保留 BOM），字体按文件实际 family 使用，不因未使用样式或零时长事件猜补系统字体。映射、ASS 和字体字节均参与存档身份，所有资源合计有界。

字幕时间来自 `movie.position_us`，storage 来自实际解码视频，raster/偏移来自视频 viewport；颜色使用原生 swscale 实际转换的 matrix/range 和 BGRA8 的 SDR 呈现策略，不按分辨率补猜。显式 HDR、未支持转换仍报 `SHINKI_EMU_MUSICA_SUBTITLE_COLOR`。内容变化才上传 Alpha8 mask，位置变化不重传；mask revision 与轨道 serial 避免 ID 冲突，视频双槽 ID 保证单帧不同时释放/重传同一资源。暂停不推进时钟；读档、seek、换片、跳过和结束释放旧轨道。原生 handles 始终在 session 线程创建、使用和销毁。

Yorino 的带括号姓名由核心根据实际 SC speaker 生成。补丁只写资源，`yorino.exe` 原样复制，不再修补 EXE 常量或括号。`script_directory` 下的 `shinki-names.sc` 使用两条原生 SC 命令：`.pragma shinki_name_unknown \u{...}` 和 `.pragma shinki_history_name_unknown \u{...}`，各自携带完整 Unicode scalar 字面量（例如 `\u{ff1f}\u{ff1f}\u{ff1f}` 和 `\u{3010}\u{ff1f}\u{ff1f}\u{ff1f}\u{3011}`）。资源优先；不存在时只读 `yorino.exe` 文件偏移 `0x112098`、`0x121f18` 的原始 CP932 常量。已存在但损坏的资源报错，不隐式回退。其他游戏没有资源和 Yorino EXE 时保留原行为。

静态分析确认实时姓名按原始首字节分类：`@` 显示未知姓名，`#` 不显示姓名，普通名取最后一个 `=` 后缀。历史空名保持空，`@`/`#` 使用完整历史未知姓名，其余保留原始全串并按脚本编码生成 `【speaker】`，不沿用 EXE 的 CP932 括号字节。实时显示、原生存档恢复与 backlog 复用同一组规则。姓名加载结果进入 VM 脚本身份与运行状态；资源 hash 进入 game 身份，支持的原生存档槽隔离到 `name-resources/<hash>/`，未打补丁的槽目录不变。描述符声明 `CAP_NATIVE_NAME_RESOURCES`（512）；Yorino 补丁要求 `256 | 512 = 768`。

普通实时姓名保留空格、Tab 和全角空格；绘制文本按脚本编码限制为前六字节内的完整字符，不切断 GBK/CP932 双字节字符。该截断只影响实时文本，历史和存档保留原始 speaker；ANI 仍使用既有独立规范化姓名匹配，不受未知姓名或历史括号影响。panel 特殊模式与姓名图集继续沿用核心现有实现，本次不扩展其绘制行为。



## 私有配置与存档

`mount_musica(game_root, profile_path)` 只读取不超过 1 MiB 的 UTF-8 TOML，按原始文件字节 SHA-256 绑定私有配置身份。`MusicaKeyFile` 使用 `shinki.emu.musica.keys.v2`，必须明确声明 `paz_version`、`index_size_xor`、`rc4_skip_crc` 和八个固定 role 的 `archive_keys`。`rc4_skip_crc` 控制 PAZ v2 的 entry CRC32 skip，缺失算法参数直接拒绝，不补默认值。配置包含私有解密信息，不进 Git、日志或报告；读取失败不写文件。

原生 reader 同时接受旧 schema 标识 `astra.emu.musica.keys.v2`，仅在内存中规范化为 `shinki.emu.musica.keys.v2`，不改写用户的原始 `key.toml`，配置身份仍绑定原始文件字节。Manager 读取和补丁转换共用该 reader，无需用户手动转换这个 v2 schema 标识；新生成的 key 使用 canonical schema。

每个 `MusicaArchiveKey` 声明偶数长度的 `index_key_hex` 和 `data_key_hex`，接受大小写十六进制；mov 允许空 data key。可选字段保留各 role 的 `type_passwords`、`archive_xor` 和 256 字节 `video_key_hex`，`texture_overrides` 仍是整份配置的可选映射。`MusicaKeyFile::validate` 纯校验 schema、算法参数、密钥和路径语法；`load_musica_key_file` 有界读取普通文件并校验，不初始化游戏或设备。TOML 错误不回显源行、密钥或路径。

PAZ 探测只识别引擎，不能据此得出密钥已安装。Manager 只选择现有 key.toml，并将明确路径保存到当前游戏的 `profile_file`；JSON profile、GARbro 方案和旧 `keys.v1` TOML 的转换只由开发 CLI 提供。运行时不导入这些不同格式；上述两个 v2 schema 标识由同一原生 parser 读取，不做格式猜测。转换说明见 [CLI](../../Programs/shinki-emu-musica-cli/README.md)；原游戏、密钥与存档保持不变。

同一 key.toml 可以可选地声明 `texture_overrides`。它是 `musica:/` 原资源 URI 到游戏目录内安全相对 PNG 路径的显式映射，例如 `musica:/bg/BG.png` 到 `hd/BG.png`。启动先验证原资源、帧数和映射路径，首次使用先解码原资源；逻辑尺寸与 ANI 原点始终来自原资源，替换图只提供物理像素。静态 PNG 与单帧 ANI 支持 PNG 替换；多帧 ANI、SQZ、缺失文件、非法路径、尺寸超限和解码失败都明确报错，不回退原图。原生和替换纹理共用 SDK `TextureCache`，通过不同 cache identity 区分。高清素材放在 ignored 私有目录，不进入仓库、日志或存档。

PAZ 的边界/重叠校验、分卷读取、Blowfish/RC4、XOR、解压和源文件变化校验保留。SDK 的 `ArchiveManifest` 是核心本地索引，不是 Host VFS 或产品 package。配置相对路径以游戏目录为基准，资源始终限于游戏目录内；公共读取错误使用 `SHINKI_EMU_PROFILE_*`。读取不运行 managed helper、BinaryFormatter 或 executable patch。

运行时统一使用原版 eden/Perseus 的 `.sav` 与同名 `.png`，路径、槽位编号、缩略图和限制见[存档说明](../../../docs/zh-cn/cores/shinki-emu-musica.md#存档)。旧 beta `.asav`（`AMUSSV06` 及更早格式）不兼容，既不读取也不转换、搬迁或删除；请保留备份并新建原生槽位。

## eden 原版存档与检查点

`eden_save::EdenSave` 提供有界容器编解码、严格检查点投影和运行时原生存读档。`EdenEdition` 显式区分日文 CD-ROM、Steam English 和 Perseus 签名；签名后依次为 NUL、有界注释、NUL、四字节 route 和单个 zlib 流。不会搜索压缩魔数来猜测偏移。

`EdenSaveEncoding` 由调用方显式选择 Shift-JIS、GBK 或 Windows-1252。解码须无替换字符，重新编码须还原原字节；写入不可表达字符直接失败。变量及 backlog 保留原字段顺序，包括多语言 `L1_0` 等字段，不补默认字段或丢弃未知字段。重复字段、结构注入、截尾、压缩校验失败、追加压缩流和大小超限均拒绝；容器和解压体各限 16 MiB。类型不实现 Debug，避免误打出正文。

格式研究参考 [ReMinori](https://github.com/luoyily/ReMinori) 的 `crates/formats/src/save.rs` 和恢复路径，固定参考 revision `79b00990324404bfbd496e7fa76c690cf040f744`，其许可证为 AGPL-3.0。本实现未引入该项目依赖或移植其代码。其原版 writer 和 port writer 是不同格式，README 的解析支持不作为原版互通结论；本地 Steam 汉化文件也不能按日文 CD-ROM 编码处理。


7 个本地原版存档副本通过严格解码及内存容器往返，覆盖 36–37 个变量、100–7590 条 backlog。七个槽的保存 PC 与消息 ID 已逐一匹配原始脚本，不进行附近搜索或位置截断。

`EdenCheckpoint` 显式保存脚本变量、背景、组合前景、立绘位置与偏移、BGM、消息历史和小说整页。`MusicaVm::from_eden_save` 读取实际脚本并按当前语音设置准备候选；`restore_eden_save` 不执行保存点之前的命令。小说页保留 `CP1=2` 和 `L1_n..L4_n`，默认 panel 2 使用 `fullPanel.png`。活动效果、旋转、滤色及尚未支持的附加图层状态明确拒绝，无内部快照回退。设备或原版游戏启动不属于本次构建与私有存档语义检查。


## Musica 成果整合

`.select` 支持 1–4 个 `display:label` 对，原始字节保留以支持源码往返；每个目标标签在解析时校验。VM 持有选择等待和焦点，GPU 文字层显示选项，上下键循环切换，Enter/Space 确认当前项。鼠标悬停更新焦点，主键只确认命中项；命中区域与显示共用布局。选择开始前触发原生自动存档，记录此前的消息边界，原生槽不保存选择焦点。

已吸收来源分支未提交修改中的 `.chain file.sc#label` 语法，合入同一个 Musica parser、VM 和原生 session。文件与标签分别校验，标签在目标脚本中解析成功后才切换脚本；缺标签保持原 VM 状态。省略标签仍从脚本起点执行，存档使用现有目标脚本身份和剧情位置，不新增存档格式。

`playbgm2` 使用独立 stream 5，`playse4` 使用 stream 6，分别沿用既有 BGM/SE 参数与播放路径；停止第二 BGM 不影响第一 BGM 和第四 SE。`deletevar` 删除同名局部及全局变量，不存在的名字按来源语义不改变状态。音频指令集中在 `runtime/audio_commands.rs`，两个新增通道共用混音、淡入淡出和存档结构。其余 Musica 执行、演出及系统功能仍待逐项整合，来源工作树保持原状。

`CrossFade` 与 `CrossFade2` 共用现有时序及 GPU 呈现。仅给出效果名称，或使用来源支持的单独 `*` 资源形式时，显式清除当前效果并重绘。原生 Family session 的 GPU fixture 已验证旧效果帧移除；时间推进和存档回归验证清除状态不会重新出现。该测试不替代真实游戏演出验收。

## 音频边界

快照或恢复请求超时会标记音频会话失败、取消 PCM sink 并停止处理后续命令；关闭仍须等待 worker 结束。恢复构建新的混音器后，在替换前再次检查取消，空恢复也不能绕过。阻塞 PCM 的实际超时回归与取消恢复回归已通过。

完整浮点解码已移入 SDK 的可选 `audio` feature，复用 Symphonia 与 Kira，保留帧预算和取消检查。核心继续持有混音、播放位置、淡入淡出及 worker 生命周期；公共解码错误使用 `SHINKI_EMU_AUDIO_*`。

Symphonia 在核心 audio worker 内完整解码，Kira 执行 resampling、混音、pan、loop 和 fade，输出固定 48 kHz stereo F32 PCM，由核心持有的 SDK `NativeAudioOutput` 转换并写入真实设备有界队列；设备不可用时明确失败。每个资源输入最多 64 MiB，总 decoded frame budget 为 32 Mi frames（256 MiB），最多 64 个 stream；超限明确失败。当前采用完整资源解码，尚不是长媒体流式 decode。Snapshot/restore 走有界 worker 命令，close 先取消阻塞写入再 join，不使用 detached decoder thread。

## 验证与剩余边界

```bash
cargo test -p shinki-emu-musica --all-features
cargo clippy -p shinki-emu-musica --all-targets --all-features -- -D warnings
cargo build -p shinki-emu-musica --features dynamic-plugin-export
```

测试覆盖 archive、图像、parser/VM、native codec 和 profile 边界。创建真实 session 的测试现在需要硬件 GPU；独立文字 GPU 测试显式使用 `--ignored` 运行，覆盖共享字形、移除区域、清空后重建与重复帧。公开最小字节格式 fixture 贯通 encrypted PAZ、SC、PNG、完整 PCM decode、Kira stereo pan、消息物理输入、atomic save/load 相同帧、translation cancel/load 和 close/reopen；单独验证阻塞 audio sink 退出、decode budget 与非挂起 VM 循环边界。Fixture audio sink 不是真实音频设备，动态模块编译也不代表 Manager 内实际安装验收。

作品兼容性与真实设备验收单独记录。标题、剧情、选择、系统页、ANI/SQZ、影片及自有存档已有实现与局部回归；完整机制、真实设备音视频、结局、Android 和跨平台仍待验收，未支持命令继续明确失败。

VM 的可序列化状态集中在 runtime/model.rs，演出命令在 runtime/effects.rs，音频命令在 runtime/audio_commands.rs，选择处理在 runtime/choices.rs。调度与存读档保留在 runtime.rs，共享演出序号仍由 VM 分配。运行错误通过 typed diagnostic_code() 返回稳定标识；未实现命令只向 Family/Manager 返回指令序号，不透传脚本文本。


原版历史 `CT5` 保存面板切换参数，支持已经核对的 0 和 10；其他值明确拒绝。逻辑画布与 GPU 光栅尺寸分别配置，eden 原版导入要求 1024×640；核心槽保存逻辑画布身份，不同画布加载在修改活动状态前拒绝。

最终合成 RGB 的 Family 帧声明 Ignored alpha，保留原字节与存档/VM 几何，发布时不重复 Opaque alpha 扫描。Host 的 GPU 归一化仍有成本；构建或元数据更新不替代演出、保存与几何验收。

SDK 的 `runtime` 只提供进程内 Rust 值；`native` 复用 C ABI 配置校验、排队输入转换、窗口服务失效与独立 CPU 租约。不再提供旧 Family 模块、公共 advance 或跨协议兼容层。`build.rs` 为实际库生成 `core-build.json` 与许可证清单。
