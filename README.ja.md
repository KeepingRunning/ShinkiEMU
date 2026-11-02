<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme-banner-dark.png">
  <img src="docs/assets/readme-banner-light.png" alt="ShinkiEMU のバナー：製品名 ShinkiEMU とキャッチコピー「Game library manager and native cores for visual novel engines」、Project-Shinkirou の楼閣マーク、水面に映る地平線上の楼閣。">
</picture>

# ShinkiEMU

ShinkiEMU は、旧来のエンジンで作られたビジュアルノベルを今のパソコンやスマートフォンで動かします。Manager アプリひとつでゲームライブラリを管理し、エンジンごとのネイティブコアをインストールし、Bangumi と VNDB からメタデータを取得し、Steam・DLsite・DMM で購入済みのゲームをダウンロードできます。

[![CI](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml/badge.svg)](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Project-Shinkirou/ShinkiEMU?include_prereleases&sort=date)](https://github.com/Project-Shinkirou/ShinkiEMU/releases)

[English](README.md) · [简体中文](README.zh-CN.md) · 日本語

ShinkiEMU はプレリリース段階で、ビルドは beta として公開しています。ビルドが成功しても、個々のゲームが遊べることの確認にはなりません。ゲームごとの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)で確認してください。

**ポータル：**[ダウンロード](https://project-shinkirou.github.io/ja/shinkiemu/#download) · [互換性](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/) · [コア](https://project-shinkirou.github.io/ja/shinkiemu/cores/) · [パッチ](https://project-shinkirou.github.io/ja/shinkiemu/patches/) · [よくある質問](https://project-shinkirou.github.io/ja/shinkiemu/faq/) · [プロジェクトの状況](https://project-shinkirou.github.io/ja/status/)

## 互換性

ShinkiEMU は 11 のコアで 10 のエンジンファミリーに対応しています。各コアはエンジン本来の VM、描画、音声、メニュー、セーブを保持します。9 つのコアは既存のオープンソースのエンジン再実装に載せたアダプター（[謝辞](#謝辞)を参照）で、Musica と CMVS は ShinkiEMU 独自の実装です。コアの一覧は [`build/cores.toml`](build/cores.toml) で定義しています。

| コア | エンジンファミリー | コアが読み込むもの |
|---|---|---|
| [`rfvp`](docs/ja/cores/rfvp.md) | FVP | HCB バイトコード |
| [`siglus_rs`](docs/ja/cores/siglus-rs.md) | SiglusEngine | `Scene.pck` シーンパッケージ |
| [`sena-rs`](docs/ja/cores/sena-rs.md) | SoftPAL | `SCRIPT.SRC` バイトコードと付随する `.DAT` テーブル。対象はオリジナル版と Steam 版の Koikake |
| [`ethornell-rs`](docs/ja/cores/ethornell-rs.md) | BGI / Ethornell | BCS シナリオ、BP システムプログラム、DSC 圧縮のアーカイブエントリ |
| [`ReMinori`](docs/ja/cores/reminori.md) | Musica | PAZ アーカイブ。eden* を基準に実装 |
| [`shinki.emu.musica`](docs/ja/cores/shinki-emu-musica.md) | Musica | PAZ アーカイブと SC スクリプト。オリジナルの eden/Perseus `.sav` セーブを使用 |
| [`art3m1s-core`](docs/ja/cores/art3m1s-core.md) | Artemis | `system.ini`、PFS（PF6/PF8）アーカイブ、番号付きパッチボリューム `.pfs.NNN` |
| [`renpy`](docs/ja/cores/renpy.md) | Ren'Py | 公式の Ren'Py 8.5.3 ランタイムを実行 |
| [`shinki.emu.cmvs`](docs/ja/cores/shinki-emu-cmvs.md) | CMVS | CPZ5 アーカイブと PS2A スクリプト |
| [`kirikiroid2-web`](docs/ja/cores/kirikiroid2-web.md) | KiriKiri | XP3 アーカイブと KAG/TJS スクリプト |
| [`impacto`](docs/ja/cores/impacto.md) | MAGES. | MAGES. SC3 スクリプトとゲームごとの設定 |

エンジンを認識できても、そのゲームが遊べるとは限りません。ゲーム・プラットフォーム・リリースごとの結果はポータルの[互換性ページ](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)にあり、レポートのないゲームは未報告です。結果を追加するには[ゲーム互換性報告](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml)フォームを使ってください。

## 拡張性

- **コアはプラグイン**：コアは Manager とは別にリリース・更新されます。デスクトップ版 Manager はアプリ内のコアカタログからコアをインストール・更新し、Android と iOS の Manager はコアを静的リンクします。
- **サードパーティのコアは上流に従う**：外部コアは組織のフォークに載せた薄いアダプターで、セーブ、鍵、設定は上流の場所と形式を使います。現在のコアパッケージは、ビルド元の上流リリースにちなんで名付けられています（例：rfvp `0.7.0`、impacto `0.10.8.930`、Ren'Py `8.5.3.26051504`）。これらのパッケージには Manager beta.6 以降が必要で、旧 Manager は従来のカタログを使い続けます。
- **パッチ**：ポータルの[パッチライブラリ](https://project-shinkirou.github.io/ja/shinkiemu/patches/)では、レビュー済みの Luau レシピを公開しています。レシピは選択したときだけ実行されます。既定では新しいディレクトリに出力し、元のゲームは変更しません。上書きモードでは変更したファイルを `<game>/.shinki/backup/` に退避し、直近のパッチを元に戻せます。ホストされたパッチファイルは HTTPS でダウンロードし、サイズと SHA-256 を検証します。新しいパッチは[パッチの提出](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml)フォームから投稿できます。
- **ストアからのダウンロード**：Manager の中で Steam、DLsite、DMM/FANZA にログインし、アカウントが所有する PC ゲームを選んでライブラリにダウンロードできます。Steam では更新の確認とインストール済みファイルの検証もできます。ログイン情報はプラットフォームの安全な保管領域にだけ保存します。DRM とライセンス確認はストア側の責任です。

それぞれの詳細は[ユーザーガイド](docs/ja/user-guide.md)にあります。

## 移植性

| コア | Windows x64 | Linux x64 | macOS x64/arm64 | Android | iOS | OpenHarmony |
|---|---|---|---|---|---|---|
| `rfvp` | ✓ | ✓ | ✓ | 組み込み | 組み込み | — |
| `siglus_rs` | ✓ | ✓ | ✓ | 組み込み | 組み込み | — |
| `sena-rs` | ✓ | ✓ | ✓ | — | 組み込み | — |
| `ethornell-rs` | ✓ | ✓ | ✓ | — | 組み込み | — |
| `ReMinori` | ✓ | ✓ | ✓ | — | 組み込み | — |
| `shinki.emu.musica` | ✓ | ✓ | ✓ | 組み込み | 組み込み | 組み込み |
| `art3m1s-core` | ✓ | ✓ | ✓ | 組み込み | — | — |
| `renpy` | ✓ | ✓ | ✓ | 組み込み | — | — |
| `shinki.emu.cmvs` | ✓ | ✓ | ✓ | — | — | — |
| `kirikiroid2-web` | ✓ | ✓ | ✓ | 組み込み | — | — |
| `impacto` | ✓ | ✓ | ✓ | 組み込み | — | — |

✓ はデスクトップ版のコアカタログで公開済みであることを示します。「組み込み」は [Manager 0.1.0-beta.7](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.7) に組み込まれたコアで、Android は 7 つ、iOS は 6 つ、OpenHarmony は Musica のみです。Android と iOS の Manager は組み込みのコアを使い、デスクトップ版はカタログからダウンロードしたコアを読み込みます。

- **Windows、Linux、macOS**：x64 向けの ZIP パッケージ。macOS には Apple Silicon 版もあります。
- **Android**：リリース署名鍵で署名したユニバーサル APK がひとつ。debug 署名の CI APK はテスト用です。
- **iOS**：実機向けとシミュレーター向けのパッケージを、テストビルドとして公開しています。
- **OpenHarmony**：OpenHarmony SDK のテスト PKI で署名した実験的な HAP。実機での起動と製品端末へのインストールは未確認です。

セーブの扱いはコアごとに異なります。外部コアは上流のセーブ場所と形式を使います。Musica はオリジナル形式の eden/Perseus `.sav` を `<game>/.shinki/musica/` に保存します。ShinkiEMU 自身がゲームディレクトリに書くファイルは、すべて `<game>/.shinki/` の下に置きます。セーブをバックアップする前や、対応するオリジナル形式のセーブファイルをやり取りする前には、ゲームを終了してください。

## ダウンロードとクイックスタート

1. [ポータル](https://project-shinkirou.github.io/ja/shinkiemu/#download)または [Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases) から自分のプラットフォーム用の Manager パッケージをダウンロードします。
2. 展開して `shinki-emu-manager`（Windows では `shinki-emu-manager.exe`）を起動します。Linux では、[ビルドとリリース](docs/ja/build-and-release.md)の Linux デスクトップの節にあるランタイムライブラリを先にインストールします。
3. 設定でゲームの入ったディレクトリを追加します。
4. コアカタログから、ゲームに必要なコアをインストールまたは更新します。
5. ライブラリでゲームを選んで起動します。

Manager の表示言語は簡体字中国語、英語、日本語です。設定、セーブ、メタデータ、ストアからのダウンロード、パッチについては[ユーザーガイド](docs/ja/user-guide.md)を参照してください。リリースノート：[Manager beta.7](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.7) · [beta.6](docs/ja/releases/manager-0.1.0-beta.6.md)

## 問題の報告

よくある質問は[よくある質問](https://project-shinkirou.github.io/ja/shinkiemu/faq/)のページにまとめています。それ以外は issue フォームを使ってください。

- [バグ報告](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=bug.yml)
- [ゲーム互換性報告](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml)
- [パッチの提出](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml)

外部エンジン自体の問題は上流に報告するほうがよい場合もあります。各コアのページに上流プロジェクトへのリンクがあります。

## 開発

プルリクエストの前に [CONTRIBUTING.md](CONTRIBUTING.md) を読んでください。ビルド、パッケージ作成、リリースの手順は、プラットフォームごとの前提条件と固定ツールチェーンも含めて[ビルドとリリース](docs/ja/build-and-release.md)にあります。新しいコアを書く場合や、既存のエンジンをコアとして取り込む場合は、[`.agents/skills/`](.agents/skills/) のエージェントスキルに従ってください。商用ゲームのデータはコミットせず、テストには自作のフィクスチャだけを使います。

| | 简体中文 | English | 日本語 |
|---|---|---|---|
| 概要 | [index](docs/zh-cn/index.md) | [index](docs/en/index.md) | [index](docs/ja/index.md) |
| ユーザーガイド | [user-guide](docs/zh-cn/user-guide.md) | [user-guide](docs/en/user-guide.md) | [user-guide](docs/ja/user-guide.md) |
| コア ABI | [core-abi](docs/zh-cn/core-abi.md) | [core-abi](docs/en/core-abi.md) | [core-abi](docs/ja/core-abi.md) |
| ビルドとリリース | [build-and-release](docs/zh-cn/build-and-release.md) | [build-and-release](docs/en/build-and-release.md) | [build-and-release](docs/ja/build-and-release.md) |
| メタデータカタログ | [metadata](docs/zh-cn/metadata.md) | [metadata](docs/en/metadata.md) | [metadata](docs/ja/metadata.md) |
| ヘッドレスランナー | [headless](docs/zh-cn/headless.md) | [headless](docs/en/headless.md) | [headless](docs/ja/headless.md) |

## 謝辞

外部コアは、以下のオープンソースのエンジンプロジェクトの上に載せた薄いアダプターです。各プロジェクトの作者とコントリビューターに感謝します。Musica（`shinki.emu.musica`）と CMVS（`shinki.emu.cmvs`）のコアは ShinkiEMU 独自の実装で、上流プロジェクトはありません。

| コア | エンジンファミリー | 上流プロジェクト | 作者・組織 | ライセンス |
|---|---|---|---|---|
| `rfvp` | FVP | [xmoezzz/rfvp](https://github.com/xmoezzz/rfvp) | xmoe（[@xmoezzz](https://github.com/xmoezzz)） | [MPL-2.0](https://github.com/xmoezzz/rfvp/blob/99435f22e94eaec27c0c1b9f22cc2a6b1910d7d1/LICENSE) |
| `siglus_rs` | SiglusEngine | [xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs) | xmoe（[@xmoezzz](https://github.com/xmoezzz)） | [MPL-2.0](https://github.com/xmoezzz/siglus_rs/blob/5f358ef1360b0439e92aed2b749e8f25d8721954/LICENSE-MPL-2.0) |
| `sena-rs` | SoftPAL | [xmoezzz/sena-rs](https://github.com/xmoezzz/sena-rs) | xmoe（[@xmoezzz](https://github.com/xmoezzz)） | [MPL-2.0](https://github.com/xmoezzz/sena-rs/blob/7f633dbb89408297827d6c1857eb8e97dc363dff/LICENSE-MPL-2.0) |
| `ethornell-rs` | BGI / Ethornell | [xmoezzz/ethornell-rs](https://github.com/xmoezzz/ethornell-rs) | xmoe（[@xmoezzz](https://github.com/xmoezzz)） | MPL-2.0（[workspace manifest](https://github.com/xmoezzz/ethornell-rs/blob/b5de417e4cff681bd2afb929e22e903d64602456/Cargo.toml) で宣言） |
| `ReMinori` | Musica | [luoyily/ReMinori](https://github.com/luoyily/ReMinori) | [@luoyily](https://github.com/luoyily) | [AGPL-3.0](https://github.com/luoyily/ReMinori/blob/79b00990324404bfbd496e7fa76c690cf040f744/LICENSE) |
| `art3m1s-core` | Artemis | [Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core) | Alphaly（[@Alphaly2K](https://github.com/Alphaly2K)） | [MPL-2.0](https://github.com/Alphaly2K/art3m1s-core/blob/ad90a62cb192b119b7cf8589aa7f87a94bd747c8/LICENSE)、`crates/pf8` は [MIT](https://github.com/Alphaly2K/art3m1s-core/blob/ad90a62cb192b119b7cf8589aa7f87a94bd747c8/crates/pf8/LICENSE) |
| `renpy` | Ren'Py | [renpy/renpy](https://github.com/renpy/renpy) | The Ren'Py Visual Novel Engine Project（[@renpy](https://github.com/renpy)）、ソースの著作権者 Tom Rothamel | [MIT（一部 LGPL）](https://github.com/renpy/renpy/blob/39895c1e017f0b36ffea2447d97eccd69d76ee1c/sphinx/source/license.rst) |
| `kirikiroid2-web` | KiriKiri | [fenghengzhi/kirikiroid2-web](https://github.com/fenghengzhi/kirikiroid2-web)（[2468785842/krkr2](https://github.com/2468785842/krkr2) のフォーク） | [@fenghengzhi](https://github.com/fenghengzhi)、KiriKiri2 の著作権者 W.Dee and contributors | [KiriKiri2 ライセンスと同梱コンポーネントのライセンス](https://github.com/fenghengzhi/kirikiroid2-web/blob/2028cbccaa91498bd36f270d349ae711d0e8fed3/LICENSE) |
| `impacto` | MAGES. | [CommitteeOfZero/impacto](https://github.com/CommitteeOfZero/impacto) | Committee of Zero and contributors（[@CommitteeOfZero](https://github.com/CommitteeOfZero)） | [ISC](https://github.com/CommitteeOfZero/impacto/blob/3706ddd89d4ad4aed0e32e127355c4251f22e574/LICENSE) |

ライセンスのリンク先は [`build/cores.toml`](build/cores.toml) で固定した上流のリビジョンです。コアと依存ライブラリの完全な告知は [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) にあります。

## ライセンス

プロジェクト独自のコードは [MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のデュアルライセンスで、どちらかを選べます。外部コア、依存ライブラリ、素材はそれぞれのライセンスに従います。[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) と [ORIGINS.md](ORIGINS.md) を参照してください。各コアのリリースには、対応するソースを収めた sources アーカイブが付きます。

ShinkiEMU の共有ライブラリは、別プロジェクトの制作エンジン [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine) から来ています。
