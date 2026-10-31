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

## パッチの種類と許諾

パッチの元記録は `Metadata/patches/entries/<id>.json` にあり、生成されるカタログは `shinki.emu.patches.v1` を使います。id、semver バージョン、`title`/`summary`（`zh-CN` と `en` は必須、`ja` は任意で、ない場合ポータルは `en` を表示します）、作品とリリースの識別子、正確なコア識別子、種類、ライセンスを記載します。必要に応じて HTTPS の出典、許諾リンク、issue への参照を添えてください。

- **recipe（レシピ）**は `Metadata/patches/` 配下の安全な相対パスにある `.luau` スクリプトを参照します。スクリプトはリポジトリ内のレビュー済みの内容でなければなりません。実行可能なメニューに入るのはレシピだけで、実行はユーザーが明示的に選んだときに限られます。
- **external（外部）**は添付ファイルまたは出典の URL を提供します。リンクやダウンロードとして表示するだけで、製品が実行することはありません。
- **hosted（ホスト）**には、公開 HTTPS の許諾根拠と、URL、SHA256、バイト数、安全なファイル名を含む添付ファイル記録が必要です。URL は `https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/` 配下に限られます。添付ファイル 1 件あたりの既定の上限は 1 GiB です。ライセンス名を書くだけでは、他者のパッチを再配布してよい根拠になりません。

ホストする Release の名前は `patch-<id>-v<semver>` 形式にします。添付ファイルは個別にレビューして配布し、メタデータのスナップショットには埋め込みません。新しいバージョンでは、正確なバイト列と許諾根拠を改めて紐づける必要があります。`release_ids` が空の場合は、提出者が作品単位で適用対象を宣言したことだけを意味します。画面には「対象リリース未限定」と表示し、「全リリースで検証済み」と書いてはいけません。

Manager は対象ゲームに一致する hosted の添付ファイルと、添付ファイル付きの external エントリにダウンロードボタンを表示します。リダイレクト先も HTTPS に限定し、ホストされた Release 添付ファイルには GitHub CDN の許可リストを適用します。まず `.part` ファイルに書き込み、バイト数と SHA256 を確認してから `<game>/.shinki/patch/<file_name>` にアトミックに配置します。検証済みの既存ファイルは再利用します。このディレクトリをパッチ入力に設定しますが、添付ファイル自体はスクリプトとして実行されません。その場で上書きするレシピは変更前のファイルを `<game>/.shinki/backup/<id>/` に退避し、マニフェストによる照合後に復元できます。

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

スナップショットには、`compatibility.json`、`cores.json`、`presets.json`、`patches.json`、レシピだけを含む `patches/catalog.json`、レビュー済みスクリプト、`index.json` が含まれます。実行可能なレシピカタログのタイトルには `zh-CN` を使います。コアのリリース記録は `Metadata/engines/cores/<plugin_id>/<platform>/<version>.json` から生成され、ダウンロード先は本リポジトリの Releases だけです。index には生成ファイルの SHA256、サイズ、元のコミットが記録されます。タイムスタンプには元コミットの時刻を使うため、同じコミット済みの入力からは同じバイト列が生成されます。上限は 1 ファイル 8 MiB、合計 64 MiB、4096 ファイルで、大きな添付ファイルはこの上限に含まれません。

index は `cores-v2.json`（`shinki.emu.cores.v2`）も対象とし、実際の旧エントリと外部コアの新しい `identity` を収録します。`cores.json` は v1 と公開済みダウンロードを維持し、旧 Manager が利用できます。新しい consumer は index の v2 を優先し、v2 がない場合だけ v1 を読み込みます。upstream/adapter の来歴、安全なファイル名、連番による最新選択は[ビルドとリリース](build-and-release.md#パッケージングとリリース)を参照してください。

Manager とポータルは同じ `https://project-shinkirou.github.io/data/shinkiemu/v1/` のスナップショットを読み込みます。データの唯一のソースは `Metadata/` で、互換性やコアの状態表を別に手書きすることはありません。[ユーザーガイド](user-guide.md)と[ビルドとリリース](build-and-release.md)も参照してください。
