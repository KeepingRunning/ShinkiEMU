# ShinkiEMU Manager 0.1.0-beta.8

新 Musica 补丁需要 Musica 核心 beta.5 或更高版本。Manager 会检查实际加载核心的原生能力，缺少能力时提示更新核心；Yorino 需要 256 和 512 两个能力位。

本版统一在线目录与本地 Luau/ZIP 补丁的执行、发行指纹绑定和事务。默认输出到独立新目录，也可选择原位覆盖并从备份还原。补丁只转换资源，不能修改或新增 EXE。开放 patches-v2 后，旧 Manager 继续读取保留的 v1 目录和旧脚本。

本版加入更新检查，并用编译期产品版本统一命令行、关于界面、更新请求和打包身份。beta.7 的已知问题是官方 Windows 二进制内部自报 beta.6；beta.7 标签及资产保持不变，请以发行页面识别该旧版。

各核心独立编号，不跟随 Manager 的 beta 序号。先更新需要的核心，再使用新补丁。每个平台包随附对应源码和原始依赖许可；Android 使用正式发布证书，macOS/iOS 使用 ad-hoc 签名，OHOS HAP 使用 OpenHarmony SDK 测试 PKI。设备启动验收已获豁免，OHOS 零售设备安装性未验证。
