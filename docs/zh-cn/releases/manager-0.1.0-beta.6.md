# ShinkiEMU Manager 0.1.0-beta.6

Musica 的手动存档、快速存档和自动存档改用 eden/Perseus 原生 `.sav` 槽位及配套 `.png` 缩略图。旧测试版 `.asav` 文件不兼容，仍留在原目录，不读取、转换、移动或删除。

Musica 除 Android 和实验性 OHOS 外，也静态集成到 iOS 真机目标和两种模拟器目标的 Manager。iOS 不支持单独安装动态核心。集成版本必须通过自身精确提交的构建、打包、签名、原生依赖闭包及对应源码门禁，不能用独立移植的结果替代。

Android Manager 在 ARM64 和 x86_64 上内置六核：RFVP、Siglus、Musica、Artemis、Impacto 和 Ren’Py。不包含 KiriKiri；其 Android 静态接入仍在开发，将随后续 Manager 版本交付。独立 Android 核心 ZIP 不代表 Manager 可用。本版保留既有 iOS 核心范围，不包含开发中的 KiriKiri/Ren’Py iOS 移植。

外部核心包现在保留原上游 ref/commit 和适配层身份。本版 Manager 可读取带身份的核心包及目录；beta.5 和更早版本继续使用未改动的旧核心包与旧目录。

桌面端的“选择输出位置”使用系统文件夹选择器。选择父目录后，检查界面显示的新 `localized-output[-N]` 路径；选择时不会创建目录，只有转换成功才发布结果，已被占用的路径不会覆盖。

转换结果可声明所需的核心原生能力。Manager 在新增库条目或绑定转换配置前检查实际加载的核心 descriptor，启动前再检查保留的 receipt。缺少能力时提示更新核心；格式错误或未知的能力掩码直接拒绝。旧 profile 不增加能力要求。

Sakura 配方 0.2 使用原生 GBK 脚本文字和资源文件名，并明确选择 HanaMinA 首选字体，不再使用旧运行时翻译表。它需要具备脚本存档隔离和配置字体优先能力的新 RFVP 核心。请先更新核心，再转换到新的独立目录；已有转换目录不会被改写。新脚本身份不复用旧日文版或外部翻译表版本的存档、既读记录。原字幕样式和本地字体输入保持不变。

OHOS HAP 仍为实验性 OpenHarmony SDK 测试 PKI 签名包。设备、模拟器、游戏及账户启动验收已获豁免，零售设备安装性尚未验证。每个已发布平台包均附对应源码和依赖许可。已发布 Manager beta4、beta5 的资产保持不变。
