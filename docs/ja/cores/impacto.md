---
title: impacto
description: impacto のフォーマット、ネイティブ動作、制約。
---

# impacto

`plugin_id: impacto` · エンジンファミリー: MAGES.

上流：[CommitteeOfZero/impacto](https://github.com/CommitteeOfZero/impacto)。適応 fork：[Project-Shinkirou/impacto の shinki ブランチ](https://github.com/Project-Shinkirou/impacto/tree/shinki)。

## フォーマットと実行

impacto は MAGES の SC3 VM、ネイティブのリソースリーダー、ゲーム固有の設定を保持しています。C++ の接続部分は引き続き `src/shinki` にあり、Cargo/CMake のビルドの入口は `shinki` にあります。OpenGL の合成、OpenAL の音声、FFmpeg の動画、ゲーム UI、セーブは、これまでどおりネイティブエンジンが実行します。

同梱リソースはアプリ専用領域へストリームで書き出し、1 ファイルの上限は 32 MiB です。上流の Noto CJK 太字フォント（17,032,620 バイト）を扱え、フォント全体と同じサイズのバッファは確保しません。

## ストレージ

`userconfig.toml` は Impacto のプラットフォーム設定ディレクトリに置かれます。Windows では `%APPDATA%/Committee of Zero/Impacto` です。現在の CC profile は `SaveDataType.None` を指定しているため、profile のセーブ形式やセーブファイルはありません。ネイティブセーブに対応する profile は、上流が定義する場所をそのまま使います。

旧 `save_root` はユーザー指定でした。Beta.1/beta.2 の設定を移すにはゲームを閉じ、移動先にファイルがない場合のみ `<旧 save_root>/userconfig.toml` をプラットフォーム設定ディレクトリにコピーしてください。Manager は廃止されたパスを一度表示します。旧パスを推測する自動移行や、上流の場所に書き込めない場合の代替先は追加しません。パッケージの基準パスは上流サンプルの `RootSavesDir = "./saves"` に合わせた `<resources>/saves` です。現在の CC profile はここにセーブを書きません。

## ネイティブ動作と制約

アダプターは C ABI v3 と CPU フレームの所有権を提供し、Manager 側で MAGES のスクリプトを書き直すことはしません。fork には、DDS の並行読み込み状態と動画音声の解放に関する所有権の修正が保持されています。Steam 版のフォントは、ユーザーが実際に持っているゲームの DDS から読み込み、代替リソースとして配布することはしません。

オープンソースの依存関係のパスでは、Bink DLL を同梱することも読み込むこともありません。ゲームのシステムデータのエラーと、まだ実行していないルートは、それぞれ別に記録する必要があります。query が成功しても、システム画面、動画、セーブが動作することの証明にはなりません。全体の ISC ライセンスと各コンポーネントのライセンスはどちらも保持され、バイナリを配布するには、ネイティブ依存関係とその対応ソースをすべて確認する必要があります。

ビルドとリリースの状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)、個々のリリースでの結果は[互換性レポート](https://project-shinkirou.github.io/ja/shinkiemu/compatibility/)を参照してください。[コア ABI](../core-abi.md)と[ユーザーガイド](../user-guide.md)もあわせて参照してください。
