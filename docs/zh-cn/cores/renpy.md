---
title: Ren’Py
description: Ren’Py 的格式、原生行为与限制。
---

# Ren’Py

`plugin_id: renpy` · 引擎家族: Ren’Py

上游：[renpy/renpy](https://github.com/renpy/renpy)。适配 fork：[Project-Shinkirou/renpy 的 shinki 分支](https://github.com/Project-Shinkirou/renpy/tree/shinki)。

## 格式与执行

该核心嵌入官方 Ren’Py 运行时，不把脚本转成新的 VM。脚本执行、Screen Language、ATL、GL2 合成、媒体和存档沿用官方行为。接入固定官方 SDK、CPython 3.12.7、Cython 3.2.4 和 SDL 2.0.20，DLL 校验和及来源记录在 `shinki/sdk.json`。

### Android Manager 静态接入

Android 为 `arm64-v8a` 与 `x86_64` 构建静态适配层。固定的官方配方从源码构建 CPython 3.12.8、Ren’Py、SDL2 与编解码库，不使用桌面 SDK 二进制或 RAPT 共享库。`shinki/android_runtime.py` 需要 Ubuntu 24.04，以及 `shinki/sdk.json` 指定的 NDK r29 Linux 压缩包和 Cubism 4-r.6.2 头文件 SDK。Cubism Core 由游戏提供，不随 Manager 分发。

Manager 在注册核心前设置解包后的 `cores/renpy/Resources` 路径。display 模块静态内建，通过私有 SDL2 EGL pbuffer 与 GLES 渲染，由 Manager 负责呈现。C/C++ 依赖符号与其他核心隔离；Python 资源使用清单内的纯 Python 包，不复制构建机的原生 wheel。源码包保留原生配方、补丁、Python 源码和完整 Manager/fork 源码，用于 LGPL 重链接。APK 构建、资源、原生依赖闭包与签名检查不代表设备或游戏验收。


## 存储

适配层保留 Ren’Py 8.5.3 的存档路径行为。设置 `config.save_directory` 时，运行时会同步游戏目录中的 `<game>/game/saves` 与 `%APPDATA%/RenPy/<config.save_directory>`；未设置时使用 `<game>/game/saves`。存档令牌和备份也沿用上游解析规则，不需要 Shinki 专用数据目录。

Beta.3 会在加载令牌和 persistent 前，一次性导入旧默认目录 `%LOCALAPPDATA%/ShinkiEMU/renpy/<game-hash>/saves`，逐字节复制缺失文件；任一原生存档目录已有同名文件时保留原文件。令牌和备份使用上游路径。旧文件保留，完成标记防止重新导入已删除的存档。失败时记录警告，不写完成标记，也不更换原生存储位置。

若 beta.1/beta.2 手动设置过 `data_directory`，请关闭游戏，将 `<旧目录>/<game-hash>/saves` 中缺失的文件复制到 `<game>/game/saves`；其中的 `tokens`、`backups` 子目录分别复制到上��对应目录，不要覆盖已有文件。已有安全密钥保留，Ren’Py 可能询问是否信任使用旧密钥签名的存档。Manager 会显示一次退役路径，便于查找。日志和生成缓存使用上游位置，Python 解释器仍禁写字节码。

## 原生行为与限制

运行时桥保留在驻留解释器中。原生构建产出 display 扩展和真实共享核心，包内还需匹配的 SDK DLL 与 `Resources/`。关闭和跨游戏重开继续清理解释器/原生状态，不能只复制核心 DLL。

桌面适配层支持 Windows x64/MSVC 与 Linux x64/GNU；其他目标需通过各自的原生运行时和打包检查。上游 Ren’Py 支持某个平台，不代表此适配已经移植。官方标准归档 reader 保持不变，发行版专用离线转换由已审查配方处理。不打包 Steam、Cubism、Bink、nvdrs 或可再分发 D3DCompiler DLL。

运行时包含多种许可，其中 LGPL 依赖保留对应源码和重链接义务。完整保留官方声明，不能把整个运行时标成 MIT。

构建/发布情况见[核心目录](https://project-shinkirou.github.io/zh-cn/shinkiemu/cores/)，具体发行版结果见[兼容性报告](https://project-shinkirou.github.io/zh-cn/shinkiemu/compatibility/)。另见[核心 ABI](../core-abi.md)和[用户指南](../user-guide.md)。
