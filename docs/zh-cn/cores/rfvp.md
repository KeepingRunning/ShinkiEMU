---
title: RFVP
description: RFVP 的格式、原生行为与限制。
---

# RFVP

`plugin_id: rfvp` · 引擎家族: FVP

上游：[xmoezzz/rfvp](https://github.com/xmoezzz/rfvp)。适配 fork：[Project-Shinkirou/rfvp 的 shinki-sakura-native 分支](https://github.com/Project-Shinkirou/rfvp/tree/shinki-sakura-native)。

## 格式

FVP 使用 HCB 字节码。文件首个 little-endian u32 是系统描述区偏移，代码从第 4 字节开始，不靠 magic 字符串识别。描述区包含入口、全局变量和 syscall 表。带长度的 C 字符串把末尾 NUL 计入长度；编码由核心配置明确选择。

## 原生行为与限制

fork 保留原 App、VM、GPU renderer、设备音频和存档。适配层用不可变 CPU 租约交付最终场景，Manager 不重写 syscall，也不通过宿主 tick 推进。译文直接写入 HCB，由普通脚本 reader 按明确编码读取；外挂翻译表及运行时替换已移除。`font_path` 为姓名、正文、注音和原生 UI 统一选择首选字体。movie/BGM ASS 映射保留明确字体和原媒体时钟。

Android 静态 Manager 依赖图中，RFVP 与 Siglus、Manager 共用 wgpu 30。原生 WMV 解码使用 Rust API，Manager 不启用可选的 `c-abi` 导出。

WMV/ASF/WMA 走 fork 原生影片及 decoder 路径。原生请求的字体和游戏资源仍是实际输入；识别出 HCB 不代表 syscall 已全部实现或所有发行版可用。

## 存档路径与格式

原 RFVP 引擎的编号存档使用 `<game>/save/sNNN.bin`，全局 HCB 数据使用
`<game>/save/save.bin`（`crates/rfvp/src/script/global.rs:134-136` 及
`setsumei/` 反向记录）。fork 的 `SaveItem` 在原始头部后追加自有运行时状态块，
全局文件使用 `RFVG` 封装的 `GlobalSaveDataV1`，因此两者都不是原格式的字节级实现。
fork 继续使用自己的文件名 `<game>/save/rfvp_sNNN.bin` 和
`<game>/save/rfvp_global.bin`，并保留读取旧版 `<game>/save/sNNN.bin` 的兼容路径；
这些文件属于 fork 的游戏目录存档，不迁移到 `.shinki`。

樱花萌放配方 2 输出原生 GBK 字面量，覆盖姓名、注音和长行，首选用户本地提供的 HanaMinA。变长字面量使用标准 `Jmp`/`PushString` 代码岛，原指令、调用和线程入口 PC 不变；新字面量地址形成新的既读身份。输出的 `shinki-native-script.json` 只有 version=1 和完整 HCB SHA256，不含译文或 PC 映射。核心核对实际加载的 HCB 后，将该转换产物的存档和既读隔离到 `<game>/.shinki/rfvp/<hash>/save`；标记不匹配明确拒绝启动。无标记游戏保持上述路径。旧日文版或外挂表版本的存档、既读不能沿用。请转换到独立输出目录并保留原数据；字体、字幕仍为用户本地私有输入，版权和许可不变。

同一 GBK 配置也用于 BIN 资源名称。配方将标准包名称表从 CP932 转为 GBK，要求每项编码长度不变、原生查找名称唯一、entry 和 payload 边界有效；数据偏移、大小和 payload 字节不变。无法表示、长度变化或名称碰撞均拒绝转换。所选 HanaMinA 与[公开花园字体](https://github.com/googlefonts/chinese/tree/gh-pages/fonts/HanaMin)的字节完全一致，采用 [Hanazono Font License / SIL OFL 双许可](https://github.com/googlefonts/chinese/blob/gh-pages/fonts/HanaMin/LICENSE.txt)；其他私有补丁字体沿用各自许可，不随源码分发。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
