---
title: Metadata 贡献
description: 兼容性 v4、补丁审查和生成快照。
---

# Metadata 贡献

## 提交报告

使用[兼容性或补丁 issue 表单](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new/choose)。兼容性报告需要 VNDB vID/rID、精确核心标识和版本、Manager 版本、平台、架构、变体、状态及简短复现步骤。只记录实际观察，不附游戏正文、密钥、商业资源或存档。构建报告不能生成可玩结论。

`shinki.emu.compatibility.v4` 使用完整键：

`(release_id, emulator, platform, architecture, variant)`

平台为 `windows`、`linux`、`macos`、`android`、`ios`、`ohos`；架构为 `x86_64`/`aarch64`。桌面平台使用 `desktop`，移动平台使用 `device`/`simulator`。核心标识区分大小写。状态为 `perfect`、`completable`、`flawed`、`boot_only`、`unplayable`。不同目标的记录独立保留；缺少对应记录时显示“尚无报告”。

源记录路径为 `Metadata/compatibility/entries/<vid>/<rid>/<plugin_id>/<platform>-<architecture>-<variant>.json`。旧记录缺少明确架构时，需要先找到依据，不猜测补齐后发布。

## Luau 补丁与授权

补丁源记录位于 `Metadata/patches/entries/<id>.json`。`patches-v2.json` 使用 `shinki.emu.patches.v2`，每条记录引用 `Metadata/patches/` 下安全相对路径的已审查 `.luau` 脚本。v2 不再区分 recipe/external/hosted 种类。提供 id、semver 版本、`title`/`summary`（必须含 `zh-CN` 和 `en`，可选 `ja`）、精确核心标识、来源、许可、授权及 issue 引用。

每条 `releases` 记录绑定具体 VNDB `release_id`、`vn_id`、发行标题、edition、已知游戏版本及文件指纹（`path`、`size`、`sha256`）。缺少发行信息时不猜测补齐。Manager 在创建可写事务前核对所选发行指纹。

脚本可调用 `ctx.download(url, sha256, size)`。在线条目必须在 `downloads` 声明每个允许的 HTTPS URL、SHA256 和字节数；缺省或空列表不允许下载。大小范围为 1 字节至 1 GiB。通过校验的文件按摘要命名，缓存到 `<game>/.shinki/patch/`；跳转保持 HTTPS，复用缓存前重新校验。仅填写许可证名称不能证明已获分发授权。

在线目录和本地 Luau/ZIP 共用 VM 与事务；未绑定目录的本地脚本标记为未经审核。默认输出到独立新目录，原位覆盖支持备份与哈希校验还原。资源补丁不能新增或修改 EXE。新 Musica 原生补丁要求核心 beta.5 或更高版本；Manager 检查实际 descriptor 的能力位，Yorino 需要 256 和 512。

旧 Manager 继续读取从 `Metadata/legacy` 生成的 `patches.json`、`patches/catalog.json` 和 `patches/legacy/` 下的脚本，不读取 v2 条目。

## 维护者审查

具有 write/maintain/admin 权限的维护者审查 issue 后，添加 `compat:accepted` 或 `patch:accepted`。intake 将正文作为数据处理，记录原始正文 SHA256 和 `updated_at`，通过普通 PR 提议键控记录，不执行 issue 中的脚本。正文修改后应撤销批准并重新审查，合并前再次核对绑定。审查内容包括 schema/路径一致性、重复键、交叉引用和授权依据。本地 intake 只生成候选文件及 `approval.json`，不授予批准，也不发布附件。

## 核心发行目录

发布核心后，自动化会为每个平台生成一条目录记录并创建 PR。受信任的 metadata 工作流会按仓库 schema 校验记录，并在校验通过后自动 squash 合并；门户的定时构建随后发布 Manager 读取的快照。若 PR 仍未合并，请检查 metadata 校验工作流及其权限，不要另行维护一份目录。

## 命令与生成文件

```sh
cargo xtask metadata schema
cargo xtask metadata validate
cargo xtask metadata build --root Metadata --out .work/metadata-snapshot
cargo xtask metadata resolve-core --plugin-id rfvp --platform windows-x86_64 --abi shinki.emu.native.c.v3
cargo xtask metadata intake --kind compatibility --event event.json --out .work/compat-candidate
cargo xtask metadata intake --kind patch --event event.json --out .work/patch-candidate
```

schema 默认导出到 `Metadata/schema`，规则来自 Rust DTO。修改源记录和脚本，不手写另一套 schema。validate/build 接受 `--root`；build/intake 要求输出目录尚不存在。

快照包含 `compatibility.json`、`cores.json`、`presets.json`、`patches-v2.json`、旧版 `patches.json` 和 `patches/catalog.json`、已审查的新旧脚本及 `index.json`。核心发行记录来自 `Metadata/engines/cores/<plugin_id>/<platform>/<version>.json`，下载只指向本仓库 Releases。index 记录生成文件的 SHA256、大小和来源提交，时间取源提交时间，同一已提交输入可生成相同字节。限制为单文件 8 MiB、总计 64 MiB、4096 个文件，大下载文件不计入该快照。

索引还包含 `cores-v2.json`（`shinki.emu.cores.v2`），其中既有真实的旧记录，也有新的第三方 `identity`。`cores.json` 保持旧 v1，并保留已发布下载，供旧 Manager 使用。新消费者优先读索引中的 v2，仅在 v2 不存在时读取 v1；上游/适配身份、安全文件名及按发布序号选最新的规则见[构建与发布](build-and-release.md#打包与发布)。

Manager 与门户读取同一份 `https://project-shinkirou.github.io/data/shinkiemu/v1/` 快照。`Metadata/` 是唯一数据源，不另行手写兼容性或核心状态表。另见[用户指南](user-guide.md)和[构建与发布](build-and-release.md)。

第三方完整核心标识最长 88 字节，因此索引另加 `compatibility-v5.json`（`shinki.emu.compatibility.v5`，核心版本上限 88，Manager 版本仍为 64）。新消费者优先读取此索引文件。`compatibility.json` 保持 v4，仅收录核心版本符合原有 64 字符上限的报告，确保旧 Manager 仍能读取整份快照；源报告不删改。
