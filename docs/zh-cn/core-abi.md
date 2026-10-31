---
title: 核心 ABI 与 SDK
description: C ABI v3 生命周期、owner 线程、CPU 帧租约和核心包。
---

# 核心 ABI 与 SDK

## 协议与入口

原生协议为 `shinki.emu.native.c.v3`，ABI 数值版本为 **3**。动态核心导出 `shinki_emu_query`，静态集成注册相同形状的 Query 函数。Rust 契约位于 `Source/FamilyApi/shinki-emu-abi`，生成的 C 头文件为 `include/shinki_emu.h`。C++ 使用这份头文件，不另行维护 ABI 定义。

边界只传 C 布局值，包括版本头、有界借用字节、不透明句柄、状态码和函数指针；Rust 容器及异常展开不跨 ABI。动态库及不可变 descriptor 存储保持驻留。query 接收版本和函数表输出指针；不支持的版本将输出置空。query 不执行 I/O、不启动 worker，也不安装 logger。

## 生命周期

1. **query** 取得不可变 `CoreApi` 表，检查版本、大小和必需函数指针。
2. **initialize** 在普通核心操作之前接入诊断。
3. **describe** 返回 family/plugin 标识、版本、格式、类型化配置和能力。CPU surface 是必需能力；非零渲染尺寸须由 descriptor 声明并落在范围内。
4. **probe** 检查游戏目录，`NO_MATCH` 属于正常结果。返回字符串借用模块存储，到下一次普通调用前有效，宿主先复制所需元数据。
5. **open** 校验配置并创建原生会话。失败时清理资源并返回空句柄；成功返回 session/context/output 身份和 surface 元数据。
6. **input** 将输入复制为有序拥有型事件并入队；**control** 请求暂停、恢复或已保留且可安全重试的操作。指针坐标对应当前显示帧的光栅像素及身份。
7. **state**、**frame** 读取已发布状态和画面，不推进 VM。失败会话携带类型化错误；只有核心声明安全时，宿主才提供重试。
8. **cancel** 可以并发调用，必须唤醒阻塞中的原生服务，不能排在被取消操作后面等待。
9. **close** 无论返回何种结果都消耗句柄；返回前等待 worker 和回调结束。宿主先阻止新 cancel，再等已有 cancel 返回，最后 close。
10. **extension** 协商具名、带版本的 C 表。不支持时返回 `UNSUPPORTED` 和空输出。可选 `shinki.module_owner` 为真实跨会话资源提供 owner，由模块 owner 线程创建，在所有会话结束后销毁。

普通 ABI 调用在宿主 owner 线程串行执行。核心自己的原生 owner 负责 VM 推进、渲染/音频调度及资源生命周期。Manager 只提供输入、控制和宿主服务，没有 advance 回调。取消和 epoch 校验会拒绝旧会话或旧输出代次的迟到结果。

## 转换结果兼容性

现有 `Descriptor.capabilities` 掩码增加两个可选语义位，ABI 版本 3、C 布局和 fingerprint 均不变：

- `CAP_NATIVE_SCRIPT_IDENTITY`（64）：校验显式声明的转换脚本 hash，隔离该脚本的存档与既读；没有 marker 的游戏保留原命名空间。
- `CAP_CONFIGURED_FONT_PRIORITY`（128）：配置的字体优先于脚本或恢复状态中的首选字体，包括 ruby 和 UI 文字。

Luau recipe 在输出 profile 的 `plugin_id`、`configuration` 旁声明 `required_capabilities = 192`，receipt 保留此要求。Manager 在新增导入游戏或绑定配置前，以及启动会话前，分别检查实际加载的所选核心的 `describe` 结果。自动导入还会拒绝转换完成后要求被改动的 receipt。缺少能力时拒绝操作并提示更新核心；未知要求会使 profile 无效，不会被忽略。不以版本字符串或文件名判定功能。旧 profile 省略该字段，默认要求为零；新 receipt 中的零值也省略，保持旧协议兼容。

## 文本替换等级

descriptor 能力位是唯一依据：不带 `CAP_TEXT_HOOK` 为 **none**；仅带 `CAP_TEXT_HOOK` 为 **synchronous**；同时带 `CAP_TEXT_HOOK | CAP_ASYNC_TEXT` 为 **async-v1**。单独声明 `CAP_ASYNC_TEXT` 无效。目前固定版本的九个外部核心均声明 none；外部核心可以选择同步替换，保留自己的调度方式。SDK 核心按归属划分：Musica 属于 SDK 自研核心；Artemis 即使使用 SDK 音频，仍是外部核心。CMVS 在独立工作树维护。

ABI 版本保持 **3**。`OpenRequestTextV1` 保留完整的基础 `OpenRequest`，只在末尾追加 `AsyncTextServiceV1` 指针；服务表自带版本 1 和大小。宿主将基础 header 的 size 设为扩展结构大小。核心先检查 size 是否覆盖扩展，再校验服务版本、大小和回调；空指针表示关闭替换。已有外部二进制继续使用布局不变的 `HostServices.text` 同步回调。async-v1 核心不回退到同步路径。

`submit` 在返回前复制有界文本项；完成前，`poll` 立即返回 `PENDING`。只有文字推进等待，画面、动画、输入和音频继续运行。结果保留替换语义、文本项 ID 和 `(session_epoch, context_epoch, request_id)` 身份；同一 context 内 request ID 递增。重复提交尚未确认的同一请求不会重复执行。`CAPACITY` 返回调用方缓冲区所需容量，不重复执行 provider 或扩展；完成结果保留到 `acknowledge`。`cancel` 只废弃指定请求；`invalidate` 推进 context，丢弃旧请求、上下文和 provider 缓存。回调不能等待网络或脚本执行完成。

Manager 的两条路径共用 provider 的 `AsyncTranslationService`；同步兼容回调等待同一份结果。Musica 在自己的 pump 中提交一次，使用保留的缓冲区轮询，不暂停音频。Enter、空格、主键点击或 Ctrl 可以跳过待完成翻译并显示替换输入原文；该输入会被消费，不会同时跳过这一句。快进模式也会跳过待完成翻译。跳过、读档、回溯和关闭都会丢弃迟到结果；失败重试保留文本事件和身份，不重新执行 VM。


## CPU 帧租约

Ready 状态的 `FrameSurface` 包含实际尺寸、stride、像素/颜色/行序元数据、帧身份及一个不可变 CPU 租约。接管前先校验元数据和 epoch。Pending、Unchanged 不携带像素所有权；显示时遵守声明的光栅/逻辑几何和颜色解释。

每个租约只归还一次，而且要等真实上传 API 不再读取像素。owner 和 release 回调在会话关闭后仍有效，也能在其他线程调用；归还不得访问 VM、设备、窗口、session 或宿主服务。持有帧不会延长游戏会话，也不能让核心覆盖仍在借出的像素。ABI 的 `CpuFrame` 包装器即使校验失败，也会归还已接管的租约。

## SDK 职责

`Source/SDK/shinki-emu-sdk` 提供共用机制，不另建游戏运行时。`runtime` 值只在进程内部使用；`native` 复用配置校验、epoch、取消、窗口迟到结果拒绝和独立 CPU 租约；`native_input` 有界复制输入，不调用 VM。`NativeAudioOutput`/`NativeAudioConverter` 服务于核心拥有的设备线程；`NativeAudioOutput::progress()` 按 48 kHz 来源时间轴返回已接收帧和已播放帧，已播放帧扣除了设备与重采样延迟，核心据此推进媒体时钟。`progress_reader()` 返回可克隆的 `NativeAudioProgressReader`，设备线程阻塞在写入或空闲时，其他线程也能读到同一时间轴。`write` 接受任意长度，不足一个设备块的尾部会留到下一次写入；设备线程空闲前调用 `flush()`，已接收的帧就都能播放出来（补入的静音计入已接收帧）。混音、媒体时钟、原生存档和游戏规则仍由核心负责。具体 API 见 [SDK](https://github.com/Project-Shinkirou/ShinkiEMU/tree/main/Source/SDK/shinki-emu-sdk)。

## 核心包

包内核心目录与库文件 stem 同名：

```text
<library-stem>/
  core.json
  shinki_emu_<core>.dll       # 或 libshinki_emu_<core>.so/.dylib
  build.json
  Resources/                 # 原生核心需要时提供
  licenses/
  <原生依赖文件>
```

`core.json` 使用 `shinki.emu.core_package.v1`，记录 `plugin_id`、`library`、`version`、`abi` 及文件路径/SHA256。清单覆盖除自身外的所有包文件。核心库使用新文件名前缀和 query 符号。打包检查架构、依赖闭包及许可，拒绝路径穿越、链接和大小写冲突，只发布到不存在的新目录。Manager 在安装前校验整个包，不接受旧产品协议或加载别名。命令见[构建与发布](build-and-release.md)。

新的第三方包增加 `identity`，分别记录原作者上游与组织适配仓库的来源；`version` 必须等于据此计算的安全标识。旧包不含此字段，保留原有 SemVer。新 Manager 在暂存前同时核对目录与包的 plugin、version 和 identity，并显示完整上游/适配标签；C ABI 和 descriptor 版本独立不变。详见[版本身份规则](build-and-release.md#打包与发布)。
