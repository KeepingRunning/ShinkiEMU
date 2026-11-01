---
title: ヘッドレス実行
description: コアの描画確認、入力リプレイ、画像比較、Manager UI のウィンドウなし撮影。
---

# ヘッドレス実行と Manager のスクリーンショット

`shinki-emu-headless` は Manager の C ABI v3 ローダー、ネイティブセッション、CPU フレームリースを再利用します。入力には固定済み RoukakuEngine の `roukaku-headless-protocol` にある `roukaku.user_input_sequence.v1` を使用します。全フィールドと JSON サンプルは[英語のインターフェース文書](../en/headless.md)を参照してください。

これらのコマンドは Manager `0.1.0-beta.7` より後のリリースに向けてリポジトリのソースに実装されており、beta.7 のダウンロードには含まれません。今すぐ利用する場合は、以下の手順でソースからビルドしてください。

## ビルドとコア実行

```powershell
cargo build -p shinki-emu-headless
cargo test -p shinki-emu-headless
cargo clippy -p shinki-emu-headless --all-targets -- -D warnings
shinki-emu-headless run --core core-package.zip --game-dir C:/Games/MyGame --frames 180 --capture-frame 1,60,180 --output artifacts/run
shinki-emu-headless run --core core-package.zip --game-dir C:/Games/MyGame --frames 180 --script replay.json --output artifacts/compare --baseline artifacts/run
shinki-emu-headless compare --actual artifacts/compare/frame-000180.png --baseline artifacts/run/frame-000180.png --max-channel-delta 4 --changed-pixel-ratio 0.001
```

`--core` は ZIP、展開済みディレクトリ、動的ライブラリ、ホストに静的登録された ID を受け取ります。`--no-default-features` は Manager UI を含めずにビルドします。`--features static-cmvs` は CMVS を `shinki.emu.cmvs` として、`--features static-siglus` は Manager と同じ固定版の Siglus を `siglus_rs` として組み込みます。Manager の対応コマンドは `shinki-emu-manager headless ...` で、GUI と同じ `static_cores::STATIC_CORES` 一覧を使います。

ゲーム全体を専用の一時ディレクトリにコピーしてから、そのコピーのパスをコアに渡します。ゲーム内のセーブや設定への書き込みはコピーだけを変更します。元のセーブ先は書き込み先として渡しません。大きなゲームには十分な一時領域が必要です。シンボリックリンクや特殊ファイルは拒否します。これは通常の保存先の分離であり、信頼できないネイティブコード用の OS サンドボックスではありません。ゲーム外のパスを明示した設定は一時パスに変更してください。

コアは HOME、AppData、XDG、キャッシュ、TEMP を一時領域に向けた子プロセスで起動します。ネイティブワーカー起動後にプロセス環境を書き換えません。文字列の `save_root` が宣言されている場合は、指定値を専用の一時セーブ先で置き換えます。出力先を元のゲーム内に置くことや、ベースラインと同じ出力先を使うことはできません。

owner と Rust ネイティブワーカーの既定スタック予算は 8 MiB です。デバッグビルドの起動時に大きな一時領域が必要なコアには、より大きい `RUST_MIN_STACK` を明示できます。コアが自分でスタックサイズを指定した場合は、その指定に従います。

サイズ、位置、モード、表示、最小化の要求は OS ウィンドウではなく一貫した仮想ディスプレイ状態を更新します。状態照会もこの仮想状態を返します。対話型モーダル要求は明示的に失敗し、ホストが回答を作り出すことはありません。

`--configuration` は既存の Manager `ConfigEntry` 配列です。例: `[{"id":"profile_path","value":{"String":"key.toml"}}]`。フィールド ID はコアの宣言に従います。`--startup-timeout-ms` の既定値は 30000 で、ネイティブ open の完了後から最初の完成 CPU フレームを待ちます。描画、スクリプト、動画を無効化して成功扱いにはしません。コアが必要とする GPU や音声デバイスは引き続き必要です。

## 時間、リプレイ、比較

C ABI v3 ではコアがゲーム時刻を管理し、決定論的な単一ステップ API はありません。`--frames` は最初の完成フレーム後に行う 60 Hz のホスト表示サンプル数で、ネイティブ VM のフレーム数ではありません。`--duration-ms` は最小の実行時間で、両者の長い方を使います。`acquired_frames` は実際に取得した異なる完成フレーム数です。静止画は複数のサンプルで同じ場合があります。

`--capture-frame 1,60,180` はサンプル番号、`--capture-ms 2500` は時間指定です。時間は次のサンプルに切り上げ、最後のサンプルは常に保存します。`--baseline <ディレクトリ>` は同名 PNG と比較します。`--baseline-hash sha256:<64桁>` は最終フレームの正規化画素ハッシュを検証します。

リプレイ JSON は `InputMessage` の配列です。`schema`、同じ `session`、単調増加の `sequence`、非減少の `tick`、`event` を指定します。対応イベントは `resume`、`focus`、`keyboard`、`ime_commit`、`pointer_move`、`pointer_button`、`wheel`、`advance_ticks`、`await`、`checkpoint` です。クリックは移動・押下・解放を別々に記録します。ポインター座標は 0..65535 の正規化値です。キー名には `Enter`、`KeyA`、`ControlLeft` などを使います。

`checkpoint` の `id` は画像名とベースライン名です。重複と予約接頭辞 `frame-` は使えません。`await` は `frame` の存在、または `frame.sha256` と `value_hash` の一致に対応し、`timeout_ticks` 内に成立しないと失敗します。ネイティブ時刻を巻き戻せないため `continue_at_match` は false にします。待機中も指定したサンプルの画像を保存します。読み込みと検証に十分な実行時間を指定してください。

出力は `<id>.png` と `<id>.json` です。JSON には寸法、サンプル番号、画素 SHA-256、PNG ファイル SHA-256、比較結果を記録します。コア成功時は `run.json`、失敗時は `failure.json` を追加します。画像不一致でも実際の PNG と比較結果は残ります。

画素を上から下へ並ぶ密な RGBA8 sRGB、alpha=255 に正規化してから SHA-256 を計算します。BGRA、行パディング、上下反転、線形 sRGB を処理し、ハッシュには `sha256:` を付けます。PNG エンコーダーの違いは画素ハッシュに影響しません。既定では完全一致です。許容差を指定した場合、どれかのチャンネル差が `--max-channel-delta` を超えた画素の割合を `--changed-pixel-ratio` と比較します。寸法は必ず一致する必要があります。

終了コードは 0 が成功、1 が入力・ファイル・ABI・実行・フレーム未生成エラー、2 が単独 CLI の構文エラー、3 が画像比較またはスクリプトの検証失敗です。Manager は既存の構文エラー時の動作を維持します。

## Manager UI のウィンドウなし撮影

```powershell
shinki-emu-headless ui --page library --language zh-CN --theme dark --width 1600 --height 1000 --output artifacts/ui/dark
shinki-emu-headless ui --page cores --language en --theme light --output artifacts/ui/light
shinki-emu-headless ui --page patches --language ja --theme dark --output artifacts/ui/dark
shinki-emu-headless ui --page settings --settings-category library-roots --language ja --output artifacts/ui/dark
```

ページは `library`、`cores`、`patches`、`settings`、言語は `zh-CN`（`zh-cn` も可）、`en`、`ja`、テーマは `dark` または `light` です。幅と高さは 360..8192、DPR は 1 固定です。Manager の対応コマンドは `headless-ui` です。

実際の `ManagerWindow` と `SlintManagerAdapter` を、既存 OHOS 実装と同じ `MinimalSoftwareWindow` ソフトウェアレンダラーで描画します。OS ウィンドウや実ユーザーのライブラリ、アカウント、ストア、ネットワークサービスは起動しません。

既定データは自作の表示用テキストで、商用ゲームのカバーや画像は含みません。`--demo-data` は `{"games":[{"title":"自作サンプル","description":"UI デモ"}],"roots":["C:/ShinkiEMU/Demo"]}` の形式です。文字列は表示専用で、指定ディレクトリを走査しません。出力は `<page>-<language>.png` とハッシュ sidecar、出典を記録した `.source.json` です。テーマごとに別の出力ディレクトリを使います。設定カテゴリは `appearance`、`library-roots`、`network`、`translation`、`input`、`about` です。
