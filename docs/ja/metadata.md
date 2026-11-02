---
title: Metadata への貢献
description: 互換性 v4、パッチのレビュー、生成スナップショット。
---

# Metadata への貢献

## レポートの提出

[互換性またはパッチの issue フォーム](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new/choose)を使ってください。互換性レポートには、VNDB の vID/rID、正確なコア識別子とバージョン、Manager のバージョン、プラットフォーム、アーキテクチャ、バリアント、状態、簡潔な再現手順が必要です。記録するのは実際に観察した内容だけにし、ゲーム本文、鍵、市販のリソース、セーブデータは添付しないでください。ビルドのレポートからプレイ可否の結論は生成されません。

`shinki.emu.compatibility.v4` は次の完全なキーを使います。

`(release_id, emulator, platform, architecture, variant)`

プラットフォームは `windows`、`linux`、`macos`、`android`、`ios`、`ohos`、アーキテクチャは `x86_64`/`aarch64` です。デスクトッププラットフォームは `desktop`、モバイルプラットフォームは `device`/`simulator` を使います。コア識別子は大文字と小文字を区別します。状態は `perfect`、`completable`、`flawed`、`boot_only`、`unplayable` のいずれかです。ターゲットごとの記録はそれぞれ独立して保持され、該当する記録がない場合は「報告なし」と表示されます。

元記録のパスは `Metadata/compatibility/entries/<vid>/<rid>/<plugin_id>/<platform>-<architecture>-<variant>.json` です。アーキテクチャが明記されていない古い記録は、根拠を確認してから公開してください。推測で補ってはいけません。

## Luau パッチと許諾

パッチの元記録は `Metadata/patches/entries/<id>.json` にあります。`patches-v2.json` は `shinki.emu.patches.v2` を使い、各記録は `Metadata/patches/` 配下の安全な相対パスにあるレビュー済み `.luau` スクリプトを参照します。v2 には recipe/external/hosted の種類はありません。id、semver、`title`/`summary`（`zh-CN` と `en` は必須、`ja` は任意）、正確なコア識別子、出典、ライセンス、許諾、issue への参照を記載します。

各 `releases` 記録は具体的な VNDB `release_id`、`vn_id`、タイトル、edition、既知のゲーム版とファイル指紋（`path`、`size`、`sha256`）を結び付けます。不明な版情報は推測しません。Manager は書き込み可能なトランザクションを作成する前に対象リリースの指紋を確認します。

スクリプトは `ctx.download(url, sha256, size)` を呼び出せます。オンライン記録の `downloads` に許可する HTTPS URL、SHA256、バイト数をすべて宣言します。省略または空ならダウンロードを許可しません。サイズは 1 バイトから 1 GiB です。検証済みファイルはハッシュ名で `<game>/.shinki/patch/` に保存し、リダイレクトも HTTPS に限定します。キャッシュは再検証してから使います。ライセンス名だけでは再配布の許諾になりません。

オンラインとローカル Luau/ZIP は同じ VM とトランザクションを使います。カタログに紐付かないローカルスクリプトは未レビューと表示します。既定では別の新しいディレクトリへ出力し、その場での上書きはバックアップとハッシュ照合による復元に対応します。リソースパッチは EXE を作成・変更できません。新しい Musica ネイティブパッチにはコア beta.5 以降が必要で、Manager は実際の descriptor の機能を確認します。Yorino はビット 256 と 512 の両方を必要とします。

旧 Manager 用に `Metadata/legacy` から `patches.json`、`patches/catalog.json`、`patches/legacy/` 配下のスクリプトを生成・維持します。旧 Manager は v2 記録を読み込みません。

## メンテナーによるレビュー

write/maintain/admin 権限を持つメンテナーが issue をレビューし、`compat:accepted` または `patch:accepted` を付けます。intake は本文をデータとして扱い、元の本文の SHA256 と `updated_at` を記録したうえで、通常の PR でキー付きの記録を提案します。issue 内のスクリプトは実行しません。本文が編集された場合は承認を取り消して再レビューし、マージ前に紐づけを改めて確認します。レビューでは、schema とパスの整合性、キーの重複、相互参照、許諾根拠を確認します。ローカルの intake は候補ファイルと `approval.json` を生成するだけで、承認を与えることも添付ファイルを公開することもありません。

## コアのリリースカタログ

コアをリリースすると、プラットフォームごとのカタログ記録を含む PR が自動で作成されます。信頼済みの metadata ワークフローがリポジトリの schema で記録を検証し、検証に成功すると PR を squash マージします。その後、ポータルの定期ビルドが Manager の読み込むスナップショットを公開します。PR が残っている場合は、別のカタログを編集せず、metadata 検証ワークフローと権限を確認してください。

## コマンドと生成ファイル

```sh
cargo xtask metadata schema
cargo xtask metadata validate
cargo xtask metadata build --root Metadata --out .work/metadata-snapshot
cargo xtask metadata resolve-core --plugin-id rfvp --platform windows-x86_64 --abi shinki.emu.native.c.v3
cargo xtask metadata intake --kind compatibility --event event.json --out .work/compat-candidate
cargo xtask metadata intake --kind patch --event event.json --out .work/patch-candidate
```

schema の出力先は既定で `Metadata/schema` で、ルールは Rust の DTO から生成されます。編集するのは元記録とスクリプトで、別系統の schema を手書きしないでください。validate/build は `--root` を受け付けます。build/intake の出力先ディレクトリは、まだ存在しないものを指定する必要があります。

スナップショットには `compatibility.json`、`cores.json`、`presets.json`、`patches-v2.json`、旧版の `patches.json` と `patches/catalog.json`、レビュー済みの新旧スクリプト、`index.json` が含まれます。コア記録は `Metadata/engines/cores/<plugin_id>/<platform>/<version>.json` から生成し、本リポジトリの Releases のみを参照します。index は生成ファイルの SHA256、サイズ、元コミットを記録します。時刻は元コミットから取得するため、同じ入力から同じバイト列を生成します。上限は 1 ファイル 8 MiB、合計 64 MiB、4096 ファイルで、大きなダウンロードはこの上限に含まれません。

index は `cores-v2.json`（`shinki.emu.cores.v2`）も対象とし、実際の旧エントリと外部コアの新しい `identity` を収録します。`cores.json` は v1 と公開済みダウンロードを維持し、旧 Manager が利用できます。新しい consumer は index の v2 を優先し、v2 がない場合だけ v1 を読み込みます。upstream/adapter の来歴、安全なファイル名、連番による最新選択は[ビルドとリリース](build-and-release.md#パッケージングとリリース)を参照してください。

Manager とポータルは同じ `https://project-shinkirou.github.io/data/shinkiemu/v1/` のスナップショットを読み込みます。データの唯一のソースは `Metadata/` で、互換性やコアの状態表を別に手書きすることはありません。[ユーザーガイド](user-guide.md)と[ビルドとリリース](build-and-release.md)も参照してください。

外部コアの完全な識別子は最大 88 バイトなので、index に `compatibility-v5.json`（`shinki.emu.compatibility.v5`、コア版の上限 88、Manager 版は従来の 64）を追加します。新 consumer はこの indexed file を優先します。`compatibility.json` は v4 を維持し、従来の 64 文字以内のコア版を持つレポートだけを収録するため、旧 Manager は引き続き snapshot 全体を読み込めます。元レポートは変更・削除しません。
