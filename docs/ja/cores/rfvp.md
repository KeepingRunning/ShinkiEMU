---
title: RFVP
description: RFVP のフォーマット、ネイティブ動作、制約。
---

# RFVP

`plugin_id: rfvp` · エンジンファミリー: FVP

上流：[xmoezzz/rfvp](https://github.com/xmoezzz/rfvp)。適応 fork：[Project-Shinkirou/rfvp の shinki-sakura-native ブランチ](https://github.com/Project-Shinkirou/rfvp/tree/shinki-sakura-native)。

## フォーマット

FVP は HCB バイトコードを使います。ファイル先頭の little-endian u32 はシステム記述領域のオフセットで、コードは 4 バイト目から始まります。magic 文字列による判別は行いません。記述領域にはエントリーポイント、グローバル変数、syscall テーブルが含まれます。長さ付きの C 文字列は末尾の NUL を長さに含めます。エンコーディングはコアの設定で明示的に選びます。

## ネイティブ動作と制約

fork は元の App、VM、GPU レンダラー、デバイス音声、セーブを保持します。アダプターは不変の CPU リースで最終シーンを渡し、Manager は syscall やホスト tick を再実装しません。翻訳は HCB の文字列に直接格納し、通常のリーダーが指定エンコーディングで読みます。外部翻訳テーブルと実行時の置換は廃止しました。`font_path` は名前、本文、ルビ、ネイティブ UI の優先フォントを統一します。movie/BGM の ASS は元のクロックと明示的なフォントを保持します。

Android の静的 Manager 依存グラフでは、RFVP、Siglus、Manager が wgpu 30 を共有します。ネイティブ WMV デコーダーは Rust API を使い、Manager では任意の `c-abi` エクスポートを有効にしません。

WMV/ASF/WMA は fork のネイティブの動画・デコーダーのパスで再生します。ネイティブが要求するフォントやゲームのリソースは引き続き実際の入力として必要です。HCB を認識できても、syscall がすべて実装済みであることや、すべてのリリースで動作することを意味するわけではありません。

## セーブパスと形式

元の RFVP エンジンは番号付きセーブを `<game>/save/sNNN.bin`、HCB のグローバルデータを `<game>/save/save.bin` に保存します（`crates/rfvp/src/script/global.rs:134-136` と `setsumei/` のリバースエンジニアリング記録）。fork の `SaveItem` は元のヘッダーの後に独自のランタイム状態ブロックを追加し、グローバルファイルは `GlobalSaveDataV1` を `RFVG` で包むため、どちらも元形式のバイト単位の実装ではありません。fork は独自のファイル名 `<game>/save/rfvp_sNNN.bin` と `<game>/save/rfvp_global.bin` を使い続け、古い `<game>/save/sNNN.bin` も読み込めます。これらはゲームディレクトリのセーブで、`.shinki` には移しません。

Sakura の変換レシピ 2 は名前、ルビ、長文を含むネイティブ GBK 文字列を出力し、ユーザーがローカルで提供した HanaMinA を優先します。長くなる文字列は標準の `Jmp`/`PushString` コード領域に置き、元の命令・呼出し・スレッド開始 PC は変えません。文字列の新しいアドレスは新たな既読 ID です。`shinki-native-script.json` には version=1 と HCB 全体の SHA-256 のみを記録し、訳文や PC の対応表は含めません。実際に読み込んだ HCB と照合後、変換出力のセーブと既読を `<game>/.shinki/rfvp/<hash>/save` に分離します。不一致は起動エラーとなり、マーカーのないゲームの保存先は変更しません。日本語版・旧外部テーブル版のセーブと既読は引き継げません。別の出力ディレクトリへ変換して元データを保管してください。フォントと字幕は従来の著作権・ライセンスを持つローカル入力のままです。

同じ GBK 設定は BIN のリソース名にも使われるため、標準アーカイブの名前テーブルも CP932 から変換します。各名前のバイト長が不変で、検索名が重複せず、entry と payload の範囲が正しい場合だけ変換し、データのオフセット・長さ・バイト列は変更しません。表現できない文字、長さの変化、名前の衝突はエラーです。選択した HanaMinA は[公開花園フォント](https://github.com/googlefonts/chinese/tree/gh-pages/fonts/HanaMin)と同一のバイト列で、[Hanazono Font License / SIL OFL のデュアルライセンス](https://github.com/googlefonts/chinese/blob/gh-pages/fonts/HanaMin/LICENSE.txt)です。他の私有フォントはそれぞれの条件を維持し、ソースとともに配布しません。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
