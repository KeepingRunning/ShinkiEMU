---
title: Ren’Py
description: Ren’Py のフォーマット、ネイティブ動作、制約。
---

# Ren’Py

`plugin_id: renpy` · エンジンファミリー: Ren’Py

上流：[renpy/renpy](https://github.com/renpy/renpy)。適応 fork：[Project-Shinkirou/renpy の shinki ブランチ](https://github.com/Project-Shinkirou/renpy/tree/shinki)。

## フォーマットと実行

このコアは公式の Ren’Py ランタイムを組み込んでおり、スクリプトを新しい VM 向けに変換することはしません。スクリプトの実行、Screen Language、ATL、GL2 の合成、メディア、セーブは公式の動作をそのまま使います。固定バージョンの公式 SDK、CPython 3.12.7、Cython 3.2.4、SDL 2.0.20 を組み込み、DLL のチェックサムと出所は `shinki/sdk.json` に記録されています。

### Android Manager への静的組み込み

Android は `arm64-v8a` と `x86_64` 向けに静的アダプターをビルドします。固定した公式 recipe から CPython 3.12.8、Ren’Py、SDL2、codec をビルドし、デスクトップ SDK のバイナリーや RAPT の共有ライブラリーは使用しません。`shinki/android_runtime.py` には Ubuntu 24.04 と、`shinki/sdk.json` に記録した NDK r29 Linux アーカイブおよび Cubism 4-r.6.2 ヘッダー SDK が必要です。Cubism Core はゲーム側の入力であり、同梱しません。

Manager は provider の登録前に展開済みの `cores/renpy/Resources` を設定します。display は組み込みモジュールとして残り、専用 SDL2 EGL pbuffer と GLES で描画し、Manager が表示を担当します。C/C++ の依存シンボルは他のコアから分離し、Python リソースには一覧に固定した pure Python パッケージを使います。ソースパッケージには LGPL の再リンクに必要な recipe、パッチ、Python ソースと Manager/fork の完全なソースを含めます。APK のビルド、リソース、依存関係、署名の検証と、実機やゲームの確認は別です。


## ストレージ

このアダプターは Ren’Py 8.5.3 のセーブ場所の動作を維持します。`config.save_directory` が設定されている場合、ランタイムはゲーム内の `<game>/game/saves` と `%APPDATA%/RenPy/<config.save_directory>` を同期します。未設定の場合は `<game>/game/saves` を使います。セーブトークンとバックアップも上流の解決規則に従い、Shinki 専用のデータディレクトリは使用しません。

Beta.3 はトークンと persistent の読み込み前に、旧既定の `%LOCALAPPDATA%/ShinkiEMU/renpy/<game-hash>/saves` から不足ファイルを一度だけバイト単位でコピーします。どちらかのネイティブ保存先に同名ファイルがあれば既存ファイルを優先します。トークンとバックアップは上流の保存先に移し、元ファイルは残します。完了マーカーにより削除済みセーブの再取り込みを防ぎます。失敗時は警告を出し、完了扱いにせず、保存先も変更しません。

Beta.1/beta.2 で `data_directory` を指定した場合はゲームを閉じ、`<旧ディレクトリ>/<game-hash>/saves` の不足ファイルを `<game>/game/saves` にコピーしてください。`tokens` と `backups` はそれぞれ上流の対応ディレクトリにコピーし、既存ファイルを上書きしないでください。既存の安全鍵は保持され、旧鍵のセーブでは Ren’Py の信頼確認が出る場合があります。Manager は廃止されたパスを一度表示します。ログと生成キャッシュは上流の場所を使い、Python インタープリターのバイトコード書き込みは無効のままです。

## ネイティブ動作と制約

ランタイムのブリッジは常駐するインタープリターの中に残ります。ネイティブのビルドでは display 拡張と実際の共有コアが生成され、パッケージには対応する SDK の DLL と `Resources/` も必要です。終了時やゲームをまたいで開き直すときもインタープリターとネイティブの状態を片付ける必要があるため、コアの DLL だけをコピーしても動きません。

デスクトップアダプターは Windows x64/MSVC と Linux x64/GNU に対応し、他のターゲットには個別のネイティブランタイムとパッケージ検証が必要です。上流の Ren’Py があるプラットフォームに対応していても、このアダプターがそのプラットフォームに移植済みとは限りません。公式の標準アーカイブリーダーはそのまま使い、リリース固有のオフライン変換はレビュー済みのレシピで処理します。Steam、Cubism、Bink、nvdrs、再配布可能な D3DCompiler の DLL は同梱しません。

ランタイムには複数のライセンスが含まれ、そのうち LGPL の依存関係には対応ソースの提供と再リンクの義務があります。公式の表記をすべて保持し、ランタイム全体を MIT と表示してはいけません。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
