# ShinkiEMU

ゲームライブラリ管理アプリと、旧来のビジュアルノベルエンジンで作られたゲームを動かすネイティブコア群です。

[![CI](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml/badge.svg)](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Project-Shinkirou/ShinkiEMU?include_prereleases&sort=date)](https://github.com/Project-Shinkirou/ShinkiEMU/releases)

[English](README.md) · [简体中文](README.zh-CN.md) · 日本語

ShinkiEMU はプレリリース段階で、ビルドは beta として公開しています。ビルドが成功しても、個々のゲームが遊べることの確認にはなりません。ゲームごとの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)で確認してください。

**ポータル：**[ダウンロード](https://project-shinkirou.github.io/ja/shinkiemu/#download) · [互換性](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/) · [コア](https://project-shinkirou.github.io/ja/shinkiemu/cores/) · [パッチ](https://project-shinkirou.github.io/ja/shinkiemu/patches/) · [よくある質問](https://project-shinkirou.github.io/ja/shinkiemu/faq/) · [プロジェクトの状況](https://project-shinkirou.github.io/ja/status/)

## 機能

- **Manager**：ゲームライブラリ、コア、メタデータ（Bangumi、VNDB）、ストアアカウント（Steam、DLsite、DMM）を扱うデスクトップ・モバイル向けアプリです。
- **ネイティブコア**：各コアはエンジン本来の VM、描画、音声、メニュー、セーブを保持します。Manager は共通の C ABI でコアを読み込みます（[コア ABI](docs/ja/core-abi.md)）。
- **コアの個別リリース**：コアは Manager とは別にリリース・更新され、アプリ内のコアカタログからインストールします。
- **モバイルへの組み込み**：Android と iOS の Manager は対応コアを静的リンクします。デスクトップ版はダウンロードしたコアライブラリを読み込みます。
- **外部エンジンは上流の挙動のまま**：外部コアは、既存のオープンソースのエンジン再実装を組織でフォークし、その上に薄いアダプターを載せたものです。セーブ、鍵、設定は上流の場所と形式を使います。
- **ゲームディレクトリを汚さない**：ShinkiEMU 自身がゲームディレクトリに書くファイルは、すべて `<game>/.shinki/` の下に置きます。

## 対応エンジン

| コア | エンジン | デスクトップ（動的コア） | Manager に組み込み |
|---|---|---|---|
| [`rfvp`](docs/ja/cores/rfvp.md) | FVP | Windows x64、Linux x64、macOS x64/arm64 | Android、iOS |
| [`siglus_rs`](docs/ja/cores/siglus-rs.md) | SiglusEngine | Windows x64、Linux x64、macOS x64/arm64 | Android、iOS |
| [`sena-rs`](docs/ja/cores/sena-rs.md) | SoftPAL | Windows x64、Linux x64、macOS x64/arm64 | iOS |
| [`ethornell-rs`](docs/ja/cores/ethornell-rs.md) | BGI / Ethornell | Windows x64、Linux x64、macOS x64/arm64 | iOS |
| [`ReMinori`](docs/ja/cores/reminori.md) | Musica | Windows x64、Linux x64、macOS x64/arm64 | iOS |
| [`shinki.emu.musica`](docs/ja/cores/shinki-emu-musica.md) | Musica | Windows x64、Linux x64、macOS x64/arm64 | Android、iOS、実験的 OHOS |
| [`art3m1s-core`](docs/ja/cores/art3m1s-core.md) | Artemis | Windows x64、Linux x64、macOS x64/arm64 | Android |
| [`renpy`](docs/ja/cores/renpy.md) | Ren'Py | Windows x64、Linux x64、macOS x64/arm64 | Android |
| [`shinki.emu.cmvs`](docs/ja/cores/shinki-emu-cmvs.md) | CMVS | Windows x64、Linux x64、macOS x64/arm64 | — |
| [`kirikiroid2-web`](docs/ja/cores/kirikiroid2-web.md) | KiriKiri | Windows x64、Linux x64、macOS x64/arm64（Android と iOS 向けの単体コアパッケージもあり） | — |
| [`impacto`](docs/ja/cores/impacto.md) | MAGES. | Windows x64、Linux x64、macOS x64/arm64 | Android |

この表は [`build/cores.toml`](build/cores.toml) と [`build/targets.toml`](build/targets.toml) で決まります。`cargo xtask matrix` を実行すると、すべての組み合わせと blocked の理由が表示されます。実際に公開されている成果物は[コアディレクトリ](https://project-shinkirou.github.io/ja/shinkiemu/cores/)で確認してください。実験的な OpenHarmony Manager は Musica のみを組み込みます。HAP は SDK テスト PKI 署名で、製品端末向けのプロビジョニングではありません。他の OHOS コアは blocked のままです。

## インストール

1. [ポータル](https://project-shinkirou.github.io/ja/shinkiemu/#download)または [Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases) から自分のプラットフォーム用の Manager パッケージをダウンロードします。
2. 展開して `shinki-emu-manager`（Windows では `shinki-emu-manager.exe`）を起動します。
3. Linux では、[ビルドとリリース](docs/ja/build-and-release.md)の Linux デスクトップの節にあるランタイムライブラリを先にインストールします。

公開 Android インストーラーはリリース署名鍵を使用し、debug 署名の CI APK はテスト専用です。[Manager beta.6 リリースノート](docs/ja/releases/manager-0.1.0-beta.6.md)に Android の6コア、既存の iOS 対応範囲、ネイティブ機能の要件を記載しています。upstream 識別子付きの新コアパッケージには beta.6 以降が必要です。旧 Manager は core v1 と compatibility v4 のカタログを引き続き使用します。

## クイックスタート

1. 設定でゲームの入ったディレクトリを追加します。
2. コアカタログから、ゲームに必要なコアをインストールまたは更新します。
3. ライブラリでゲームを選んで起動します。

設定、セーブ、メタデータ、パッチについては[ユーザーガイド](docs/ja/user-guide.md)を参照してください。

## ソースからのビルド

サブモジュールごとクローンし、固定された Rust 1.99.1 ツールチェーンを使います。ビルド、パッケージ、リリースはすべてリポジトリのルートで実行する `cargo xtask` コマンドです。

```sh
git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU
cd ShinkiEMU
cargo xtask doctor --target x86_64-pc-windows-msvc          # 不足している前提条件を表示
cargo xtask deps --target x86_64-pc-windows-msvc --all-cores
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --core rfvp --release
cargo xtask test --core rfvp
```

`deps` は、`build/cores.toml` で固定された正確なコミットで各外部コアのフォークを取得し、バージョン固定のネイティブ依存も準備します。プラットフォームごとの前提条件（MSVC、Linux パッケージ、Android NDK、Ren'Py SDK）、パッケージ作成、リリースは[ビルドとリリース](docs/ja/build-and-release.md)にあります。

## ドキュメント

| | 简体中文 | English | 日本語 |
|---|---|---|---|
| 概要 | [index](docs/zh-cn/index.md) | [index](docs/en/index.md) | [index](docs/ja/index.md) |
| ユーザーガイド | [user-guide](docs/zh-cn/user-guide.md) | [user-guide](docs/en/user-guide.md) | [user-guide](docs/ja/user-guide.md) |
| コア ABI | [core-abi](docs/zh-cn/core-abi.md) | [core-abi](docs/en/core-abi.md) | [core-abi](docs/ja/core-abi.md) |
| ビルドとリリース | [build-and-release](docs/zh-cn/build-and-release.md) | [build-and-release](docs/en/build-and-release.md) | [build-and-release](docs/ja/build-and-release.md) |
| メタデータカタログ | [metadata](docs/zh-cn/metadata.md) | [metadata](docs/en/metadata.md) | [metadata](docs/ja/metadata.md) |

## コントリビュート

プルリクエストの前に [CONTRIBUTING.md](CONTRIBUTING.md) を読んでください。新しいコアを書く場合や、既存のエンジンをコアとして取り込む場合は、[`.agents/skills/`](.agents/skills/) のエージェントスキルに従ってください。商用ゲームのデータはコミットせず、テストには自作のフィクスチャだけを使います。

## 問題の報告

よくある質問は[よくある質問](https://project-shinkirou.github.io/ja/shinkiemu/faq/)のページにまとめています。それ以外は issue フォームを使ってください。

- [バグ報告](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=bug.yml)
- [ゲーム互換性報告](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml)
- [パッチの提出](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml)

外部エンジン自体の問題は上流に報告するほうがよい場合もあります。各コアのページに上流プロジェクトへのリンクがあります。

## ライセンス

プロジェクト独自のコードは [MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のデュアルライセンスで、どちらかを選べます。外部コア、依存ライブラリ、素材はそれぞれのライセンスに従います。[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) と [ORIGINS.md](ORIGINS.md) を参照してください。各コアのリリースには、対応するソースを収めた sources アーカイブが付きます。

ShinkiEMU の共有ライブラリは、別プロジェクトの制作エンジン [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine) から来ています。
