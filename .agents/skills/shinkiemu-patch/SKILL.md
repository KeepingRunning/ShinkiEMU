---
name: shinkiemu-patch
description: Use when writing, reviewing, migrating or debugging a ShinkiEMU game patch - a single-category Luau recipe run by the shinki-emu-patch sandbox, bound to concrete VNDB releases (rID) and file fingerprint groups, applied from the online patches-v2 catalog or as an unreviewed local .luau script or ZIP package (optional .patch.json release sidecar), writing a new independent directory or an in-place overwrite with .shinki/backup restore, hashed HTTPS ctx.download cached in .shinki/patch, Metadata/patches/entries v2 records versus frozen legacy v1, the Luau patch issue form and metadata intake review PR, and the patch CLI (list/apply/restore). Not for core development (shinkiemu-new-core, shinkiemu-integrate-existing-core) or build/release work (shinkiemu-maintenance).
---

# ShinkiEMU 补丁

补丁只有一种：一个 Luau 配方脚本，由 `shinki-emu-patch`（`Source/Tools/shinki-emu-patch`）里的沙箱执行。在线目录里的补丁，和 Manager 里加载的本地 `.luau` 脚本或 ZIP 包，走同一个执行器。

契约以这些文件为准：`Metadata/README.md`、`Source/Tools/shinki-emu-patch/src/`（`vm.rs` 宿主 API、`release.rs` 版本识别、`download.rs` 下载、`package.rs` 本地 ZIP、`inplace.rs` 原位与还原、`transaction.rs` 事务）、`Source/Providers/shinki-emu-metadata/src/patches.rs` 里的 v2 DTO。下面的接口名都来自这些源码。

## 1. 文件布局

| 位置 | 内容 |
|---|---|
| `Metadata/patches/entries/<id>.json` | v2 条目（schema `shinki.emu.patches.v2`）：标题、版本、核心、来源、许可、摘要、`recipe.script`（`<id>.luau`）、`releases`、`downloads` |
| `Metadata/patches/<id>.luau` | 入口脚本 |
| `Metadata/patches/common/*.luau` | 共享模块（`musica.luau`、`fvp.luau`、`renpy_rpa.luau`、`pe.luau` 等），用 `ctx.load("common/<name>.luau")` 引入 |
| `Metadata/legacy/` | 冻结的 v1（`patches.json`、`patches/catalog.json` 和旧脚本），只给旧 Manager 用，不再改 |

`cargo xtask metadata build --out <新目录>` 生成快照：新 Manager 读 `patches-v2.json`（顶层 `{schema, entries}`），每个配方旁边生成 `patches/<stem>.patch.json`（发行绑定）；旧 Manager 继续读 `patches.json`（v1），旧脚本放在 `patches/legacy/`。新 Manager 不会回退到 v1。

现有配方（6 个，`downloads` 都是 `[]`）：`fvp-sakura-cn`（樱花萌放，离线转换为原生 GBK HCB 并统一字体）、`musica-eden-cn`、`musica-ef-first-cn`、`musica-perseus-cn`、`musica-yorino-cn`、`renpy-himanatsu`。新配方照着写。

## 2. 发行绑定

每个条目的 `releases` 是非空数组，最多 256 项。每项是一个具体发行：

```json
{
  "release_id": "r105139",
  "vn_id": "v22313",
  "title": { "zh-CN": "樱花，萌放", "en": "Yinghua, Mengfang" },
  "edition": "Internet download",
  "game_version": null,
  "fingerprints": [
    { "path": "Sakura.hcb", "size": 5002852, "sha256": "946877dd0ed8fbf318ba5c73d20afd46b9dcdc200aa7e8edc610c437cc1c789b" }
  ]
}
```

- `release_id`/`vn_id` 用 VNDB 现有的 rID/vID。`edition` 写真实版本名称，`game_version` 写真实游戏版本或 `null`。配方版本、游戏版本和引擎运行时版本是三个独立的身份，不要混用。
- `fingerprints` 是本地文件指纹组 `{path, size, sha256}`，sha256 为 64 位小写 hex。选能唯一区分发行的文件，通常是主脚本包或可执行文件。
- 执行前 `release::identify` 逐个发行比对指纹：先比大小，大小相同才算 SHA256。恰好匹配一个发行才继续。不匹配时报 `ReleaseMismatch`，原因写成 `<rID>: <path> (missing or unreadable file | file size differs | SHA256 differs)`；两个不同 rID 同时匹配时报 "release fingerprints are ambiguous"。两种输出模式都在识别通过之后才创建可写的事务状态。
- 作品链接或用户手动选的发行都不能代替文件指纹。目录补丁没有绑定就不能进 v2。

### 本地补丁

Manager 补丁页可以选本地文件，界面会标成未审核：

- **单个 `.luau` 脚本。** 旁边有同名 `<stem>.patch.json`（内容是上面的 `releases` 数组，不超过 1 MiB）时，先做同样的版本识别。没有 sidecar 时不做识别，脚本要自己核对输入（用 `ctx.check` 或 `ctx.fail`）。
- **ZIP 包。** 根目录必须有 `patch.luau`；其他 `.luau` 模块用 `ctx.load` 加载，资源文件用 `ctx.open("package", path)` 只读访问。压缩包不超过 64 MiB，解开后不超过 1 GiB；路径必须是安全的相对路径，不允许链接、特殊文件或大小写重复的路径。

## 3. 配方脚本

入口脚本返回一个表：

```lua
return {
    id = "musica-eden-cn",
    version = 2,                -- 正整数
    title = "eden 中文姓名资源",
    apply = function(ctx)
        return ctx.load("common/musica.luau").apply_names(ctx, { ... })
    end,
}
```

`apply(ctx)` 返回输出 profile：`{ plugin_id = "<核心 id>", configuration = { <key> = "<string>" }, required_capabilities = <整数，可省略> }`。只允许这三个字段。`required_capabilities` 只能用这几个能力位的组合：`CAP_NATIVE_SCRIPT_IDENTITY`（64）、`CAP_CONFIGURED_FONT_PRIORITY`（128）、`CAP_NATIVE_EMBEDDED_TEXT`（256）、`CAP_NATIVE_NAME_RESOURCES`（512）。Manager 会检查目标核心是否声明了这些能力，旧核心不满足就拒绝。现有配方的取值：Musica 嵌入 256，Yorino 768（256|512），RFVP 原生 GBK/字体 192。

**补丁只改资源，不改可执行代码。** 输出里的 EXE 必须和源目录同一路径的文件逐字节相同，不能新建替换用的 EXE；否则报 "resource patches must preserve executable bytes"。原位模式会保留没有写到的原 EXE。以前靠改 EXE 常量实现的功能，改成核心原生能力加资源文件：例如 Yorino 的姓名常量写进 `scripts/shinki-names.sc`（`.pragma shinki_name_unknown`、`.pragma shinki_history_name_unknown`），历史姓名括号由核心生成，所以要求能力 768。

### 沙箱

VM 只开 table、string、utf8、math、bit32、buffer、coroutine 这几个标准库，内存上限 256 MiB。`print`、`warn`、`require`、`dofile`、`loadfile`、`os`、`io`、`package`、`debug` 都被移除。没有执行进程的入口，没有任意网络访问，也不能访问授权根以外的路径；符号链接和路径逃逸会被拒绝。

### 宿主 API（`ctx`）

| 函数 | 作用 |
|---|---|
| `ctx.open(root, path)` | 只读打开输入文件，返回带 sha256 的句柄，并记进输入清单。`root` 是 `"source"`（游戏副本）、`"patch"`（补丁输入目录）或 `"package"`（本地 ZIP 包内的资源） |
| `ctx.exists(root, path)`、`ctx.list(root, path)` | 查询输入 |
| `ctx.create(path)`、`ctx.edit(path)` | 在输出暂存区新建或修改文件 |
| `ctx.copy(root, source, output)` | 把输入复制到输出 |
| `ctx.mkdir`、`ctx.remove`、`ctx.rename` | 只作用于输出暂存区 |
| `ctx.load(path)` | 加载 `common/` 等共享脚本，脚本 hash 记进回执 |
| `ctx.download(url, sha256, size)` | 下载一个文件，返回只读的输入句柄（与 `ctx.open` 返回的同类），见下文 |
| `ctx.progress(phase, completed, total)` | 报告进度 |
| `ctx.check_cancelled()` | 长循环里主动检查取消 |
| `ctx.check(label, passed, detail)` | 报告一项检查结果（Manager 进度里可见）；`passed` 为 false 时以 `UNSUPPORTED_INPUT` 中止。label 不超过 256 字节，detail 不超过 2048 字节 |
| `ctx.fail(code, message)` | 中止并回滚。`code` 取 `INVALID_REQUEST`、`UNSUPPORTED_INPUT`、`OUTPUT_EXISTS`、`PATH`、`BOUNDS`、`MEMORY`、`CODEC`、`INTEGRITY`、`CANCELLED`、`SCRIPT`、`IO`、`PUBLISH` |
| `ctx.musica_embed(profile, translations, resource_paths, destination)` | 用 Musica 原生 PAZ/SC 读取器把翻译写成原生 SC 操作数，不安装运行时翻译表 |
| `ctx.musica_profile(buffer)` | 用原生读取器解析私有 Musica `key.toml`（keys.v2，也接受旧的 `astra.emu.musica.keys.v2` 标识，只在内存里规范化，不改写用户的源文件） |
| 编解码与数据模块 | `codecs`、`data`、`pickle` 安装的格式工具 |

Luau 原生 `error(message)` 也会中止并回滚，归为 `SCRIPT`；Manager 保留错误文本并报告已回滚。要给出具体失败类别时用 `ctx.fail`。

游戏目录本身只读：脚本读到的是暂存副本，所有写入都进事务暂存区，由宿主在成功后发布。脚本出错、`ctx.check` 失败、`ctx.fail` 或取消都会丢弃暂存区；原位模式下已改动的游戏文件会恢复。

### 下载

`ctx.download(url, sha256, size)` 由 `download.rs` 实现：

- 原 URL 和每次重定向都必须是 HTTPS（最多 10 次），URL 不能带凭据。
- sha256 是 64 位小写 hex；size 在 1 字节到 1 GiB 之间。Content-Length 不符直接拒绝；没有 Content-Length 时，超过声明大小立即中止。
- 缓存在 `<game>/.shinki/patch/<sha256>.bin`。文件名只由 hash 决定，URL 和响应头不参与路径。命中缓存时重新校验大小和 hash，不发网络请求。
- 目录补丁运行时，只允许与条目 `downloads` 里某一项 `{url, sha256, size}` 三者完全一致的调用，否则报 "undeclared download"；`downloads` 为空或缺失就不允许任何下载，`downloads` 里每个 URL 只能出现一次。投稿表单的 Downloads 字段就是这份声明。本地脚本没有声明清单，不做这项限制，但 HTTPS、hash、大小和缓存规则照样生效。Manager 关闭网络时，所有补丁都不能下载。

## 4. 输出方式

模式由 Manager 或 CLI 选择（`PatchMode`），配方本身不决定：

- **新目录**（默认，`PatchMode::NewDirectory { output }`）：输出一个新的独立游戏目录，目标必须不存在，不复制存档。CLI：`shinki-emu-manager patch apply --script <file> --source <游戏目录> [--patch <补丁输入目录>] --output <新目录>`。
- **原位覆盖**（`PatchMode::InPlace`）：Manager 补丁页的原位模式，或 CLI `patch apply ... --in-place`（与 `--output` 互斥）。写入前把会被覆盖的原文件备份到 `<game>/.shinki/backup/backup-<UTC 纳秒 hex>-<pid hex>-<计数 hex>/`，目录里的 `manifest.json`（schema `shinki.emu.patch_backup.v1`）记录 `recipe_id` 和被替换的文件；目录名不按补丁 id 分层。回执写到 `<game>/.shinki/patch-result.json`。还原：Manager 里的还原按钮，或 `shinki-emu-manager patch restore --game <游戏目录>`，还原最近一次原位补丁。还原前会核对被补丁写入的文件 hash，用户在补丁后改过的文件不会被覆盖。

`shinki-emu-manager patch list [--scripts <metadata 目录>]` 每行输出一条目录记录（JSON），不执行配方。CLI 失败退出码为 1，取消为 130。

## 5. 写一个新配方

1. 确认目标发行的 VNDB rID/vID，从一份干净安装里取指纹文件的大小和 SHA256。同一补丁支持多个发行时，各写一项 `releases`，指纹必须能区分它们。
2. 能复用 `common/` 模块就复用（Musica 用 `common/musica.luau`，FVP 用 `common/fvp.luau`，Ren'Py 用 `common/renpy_rpa.luau`）。脚本里再核对一遍关键输入的大小和 hash，不一致就用 `ctx.check` 或 `ctx.fail("UNSUPPORTED_INPUT", ...)` 中止。
3. 优先离线转换成核心原生能读的格式（例如 FVP 的原生 GBK HCB、Musica 的原生 SC 操作数），不要依赖运行时翻译表或注入钩子，也不要改 EXE。核心缺少需要的原生能力时，先给核心加能力位，再在 profile 的 `required_capabilities` 里声明。存档和既读记录要和原版隔离时，在 profile 的 `configuration` 里声明。
4. 本地试跑：把脚本和 `<stem>.patch.json` 放在一起，用 `patch apply` 输出到新目录，再用 headless runner 打开输出目录确认出帧（见 `shinkiemu-maintenance` §13）。原游戏只读，试跑用副本。
5. 提交 `Metadata/patches/entries/<id>.json` 和脚本，跑 `cargo xtask metadata validate`。

不要提交游戏正文、key、商业素材或存档。翻译文本必须有许可。

## 6. 投稿与审核

- 外部投稿用 GitHub issue 表单 `.github/ISSUE_TEMPLATE/patch.yml`（https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml），说明为中英日三语。字段：补丁 id、中英标题和摘要、脚本版本、核心、发行绑定 JSON（vID、rID、发行标题、edition/版本、指纹）、来源 URL 和许可（都必填）、授权 URL（可选）、下载声明 JSON、完整 Luau 脚本。
- 维护者给 issue 打上 `patch:accepted` 后，`metadata-intake.yml` 运行 `cargo xtask metadata intake --kind patch`（实现在 `xtask/src/metadata/intake.rs`）。它只做这些：必填字段非空；rID 为 `r` 加 1–63 位数字、vID 为 `v` 加 1–63 位数字；发行绑定和指纹走 DTO 校验；下载声明严格解析为 `PatchDownload`，要求 HTTPS、64 位小写 sha256、非负整数 size，URL 不能重复；拒绝重复的表单段落、重复的 JSON key 和不安全的 id；脚本去掉 Markdown 代码块外壳后，直接调用 Luau 编译器检查语法。它不执行脚本、不访问下载 URL、不构造合成游戏输入。任何一项失败都不会写出文件。
- 通过后生成 `Metadata/patches/entries/<id>.json`（`recipe.script = "<id>.luau"`，下载声明写进 `downloads`）和 `Metadata/patches/<id>.luau`，再开一个普通 review PR，不会自动合并。issue 正文改动会撤销已有的接受标签，需要维护者重新确认。

## 完成检查

- [ ] 条目有非空 `releases`，每项 rID/vID 真实、指纹能唯一识别该发行。
- [ ] 脚本返回 `id`/`version`/`title`/`apply`；`apply` 返回合法 profile。
- [ ] 关键输入在脚本内再次核对；不匹配用 `ctx.check` 或 `ctx.fail` 中止。
- [ ] 输出里的 EXE 与源文件逐字节相同；`required_capabilities` 覆盖配方依赖的核心原生能力。
- [ ] 所有下载都在条目中声明，HTTPS、sha256、size 齐全，单个不超过 1 GiB。
- [ ] 新目录输出能用 headless runner 打开并出帧；原位模式能还原。
- [ ] `cargo xtask metadata validate` 通过；没有提交商业数据。
