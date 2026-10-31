---
title: impacto
description: impacto 的格式、原生行为与限制。
---

# impacto

`plugin_id: impacto` · 引擎家族: MAGES.

上游：[CommitteeOfZero/impacto](https://github.com/CommitteeOfZero/impacto)。适配 fork：[Project-Shinkirou/impacto 的 shinki 分支](https://github.com/Project-Shinkirou/impacto/tree/shinki)。

## 格式与执行

impacto 保留 MAGES SC3 VM、原生资源 reader 和游戏专用配置。C++ 接入仍位于 `src/shinki`，Cargo/CMake 构建入口位于 `shinki`。OpenGL 合成、OpenAL 音频、FFmpeg 影片、游戏 UI 和存档继续由原生引擎执行。

### Android Manager 静态接入

Android 构建将适配层静态链接到 Manager，覆盖 `arm64-v8a` 和 `x86_64`，不单独安装动态核心。包内的 profile、着色器和资源位于 `cores/impacto/Resources`。`resources_root` 为空时使用解包后的资源目录，用户显式指定的路径保持不变。

打包资源以流式写入应用私有目录，单文件上限为 32 MiB，可容纳上游 17,032,620 字节的 Noto CJK 粗体字库，无需分配与整个字库同大的内存。

内置 minilua 的函数及 `lua_ident` 数据符号使用 `shinki_impacto_` 前缀，与 Manager 的 Luau VM 隔离。原生依赖按 ABI 分开安装。构建、APK 依赖闭包与签名检查不代表设备或游戏验收。
## 存储

`userconfig.toml` 保留在 Impacto 的平台偏好目录。Windows 路径为 `%APPDATA%/Committee of Zero/Impacto`。当前 CC profile 将 `SaveDataType` 设为 `None`，没有 profile 存档格式或存档文件；后续支持原生存档的 profile 继续使用上游定义的位置。

旧 `save_root` 由用户指定。迁移 beta.1/beta.2 配置时，请关闭游戏，只在目标文件不存在时，将 `<旧 save_root>/userconfig.toml` 复制到平台偏好目录。Manager 会显示一次退役路径。核心不猜测旧路径，也不在上游目录不可写时另找目录。打包资源中的基础路径使用 `<resources>/saves`，对应上游示例的 `RootSavesDir = "./saves"`；当前 CC profile 不会在此写入存档。

## 原生行为与限制

适配层提供 C ABI v3 和 CPU 帧所有权，不在 Manager 重写 MAGES 脚本。fork 保留 DDS 并发读取状态与影片音频释放的所有权修复。Steam 字库从用户实际游戏的 DDS 读取，不作为替代资源分发。

开源依赖路径不打包或加载 Bink DLL。游戏系统数据错误和未运行路线仍须分别记录，query 成功不能证明系统页、影片或存档可用。整体 ISC 与各组件许可同时保留，二进制分发需要核齐原生依赖及对应源码。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
