---
title: ビルドとリリース
description: Cargo によるオーケストレーション、コアのビルドスクリプト、パッケージング、明示的な blocked 状態。
---

# ビルドとリリース

コアのウィンドウなし実行、入力リプレイ、PNG/hash 比較、Manager UI の撮影は[ヘッドレス実行](headless.md)を参照してください。

## ゲームディレクトリ内の保存先

ShinkiEMU 自身がゲームディレクトリ内に書き込むファイルは、すべて `<game>/.shinki/` 配下に置きます。製品のセーブ、キャッシュ、ログ、レシート、パッチ添付ファイル、一時ファイルを市販ゲームのルートや元のセーブディレクトリに書き込んではいけません。担当ごとの領域は Musica の `musica/saves/`、ダウンロード添付の `patch/`、トランザクションの `staging/`、元ファイルとマニフェストの `backup/<id>/`、その場で上書きしたレシートの `patch-result.json` です。新たな書き込み側もパス検証とディレクトリリンク非追跡を守ります。サードパーティ製コアは各上流の保存規約に従います。

## 入力と依存関係

リポジトリはサブモジュールを含めて再帰的にクローンし、Rust/Cargo とターゲットプラットフォームのネイティブツールチェーンを用意します。Windows のネイティブビルドには、x64 MSVC 開発者環境、CMake、Ninja、Git が必要です。Android では JDK 27、NDK 30.0.16248370、cargo-ndk 4.1.2 を使います。ツールのバージョンとダウンロード入力は `build/dependencies.toml`、コアの manifest、features、ライセンスは `build/cores.toml`、ターゲットと runner の定義は `build/targets.toml` にあります。

Ren’Py にはさらに、固定バージョンの公式 SDK が必要です。`RENPY_SDK_ROOT`、`RENPY_PYTHON_DEV`、`RENPY_SDL_INCLUDE` を設定し、CPython 3.12.7 に対応する公式の Cython 3.2.4 コンパイラーを用意してください。これらは上流のビルドと実行に必要な依存関係です。SDK の入力は fork の `shinki/sdk.json` に従います。
POSIX SDL 2.0.20 のソースは SDL 公式 GitHub Release から取得します。SHA256 は `c56aba1d7b5b0e7e999e4a7698c70b63a3394ff9704b5f6e1c57e0c16f04dd06` のままで、アーカイブの内容と SDK ABI は変わりません。Android Ren’Py のソースビルドには LLVM 18/Clang 18、LLD、ccache とネイティブ開発パッケージも必要です。CI と Release は同じ既存の依存関係一覧を使います。Android SDK/NDK だけでは足りません。


CMVS は Win32 に依存せず、Windows x64、Linux x64、macOS の両アーキテクチャ向けに動的コアとしてビルドします。Android と iOS は Manager への静的組み込みを待っています。RoukakuEngine は公開リビジョン `8392cf161800f7684af7704eb95f7a5526d2015d` に固定されています。

リポジトリのルートで実行します。

```sh
cargo xtask --help
cargo xtask matrix
cargo xtask doctor --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc
cargo xtask doctor --target x86_64-pc-windows-msvc --core renpy
```

`doctor` は不足している入力を検査し、問題があれば 0 以外で終了します。依存関係のインストールは行いません。`deps` は明示的なダウンロードと準備の入口で、`--core <plugin_id>` で範囲を絞れます。準備するのはロックされたネイティブ依存関係で、システムパッケージはインストールしません。通常のコマンドが暗黙に deps を実行することはありませんが、Cargo が解決済みの依存関係を取得できる必要はあります。

## ビルド、チェック、テスト

```sh
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --core rfvp --release
cargo xtask build --target x86_64-pc-windows-msvc --all-cores --release
cargo xtask check --target x86_64-pc-windows-msvc --manager
cargo xtask check --target x86_64-pc-windows-msvc --core rfvp
cargo xtask test --manager
cargo xtask test --core rfvp
cargo xtask test
```

build では `--manager`、`--core`、`--all-cores` のいずれか 1 つを必ず指定します。check は Manager または単一のコアを選びます。test はホスト上で実行され、`--target` 引数はありません。選択なしの test は xtask とビルド補助ライブラリのテストを実行します。ネイティブ挙動の回帰テストには、実際の GPU、音声デバイス、公開フィクスチャーが必要な場合があります。

各コアの `build.rs` は自身のターゲットと入力を検証し、Cargo の `OUT_DIR` に `core-build.json` を書き出します。C++ のアダプターはこれを通じて実際の CMake ビルドを駆動し、純粋な Rust コアはこれまでどおり Cargo がコンパイルします。ビルドスクリプトが Cargo を再帰的に実行したり、Release を公開したりすることはありません。xtask は `compiler-artifact` と `build-script-executed` の JSON から実際の成果物を読み取り、target のパスを推測しません。各コンポーネントは `.work/target/<target>/<component>` を使い、ビルド記録は `.work/build/` に書き込まれます。Artemis はすべてのプラットフォームで Luau を使用し、Android への静的組み込みでは Manager とランタイムを共有します。

Manager は、ネットワーククライアントやワーカースレッドを作る前に、プロセス全体の Rustls 暗号プロバイダーを一度だけ初期化します。選ぶのは、reqwest 0.13 とプラットフォームの証明書検証スタックに合わせた AWS-LC です。VNDB が現在使っている reqwest 0.12 は ring も有効にするため、feature による自動選択には頼れません。Steam などの provider は、プロセス全体の既定実装を自分ではインストールしません。

### コンパイル時に組み込むストア

`shinki-emu-store` はオブジェクト安全な provider/session trait、キャンセル、秘密情報ストアの境界、`Registry` を定義します。Manager の `store-steam`、`store-dlsite`、`store-dmm` feature は既定で有効です。Steam のみのビルドでは Manager パッケージに `--no-default-features --features store-steam` を指定します。ストアを追加するには provider crate と任意の Cargo 依存関係・feature を追加し、`manager_app/store.rs` に `registry.register(...)` を一行追加します。動的ライブラリのロードは行いません。Provider はプロセス内だけで使う不透明な計画、秘密情報を含まない固定の `StoreError`、所有済み作品を返します。一時ファイルには渡された `.shinki/store/<provider>/` 作業ディレクトリを使い、認証情報は `SecretStore` だけに保存します。


### Linux デスクトップ

リポジトリで固定された Rust 1.99.1 ツールチェーンを使います。Ubuntu 22.04/24.04 では、ネイティブ開発パッケージを明示的にインストールします。

```sh
sudo apt-get install -y --no-install-recommends build-essential cmake ninja-build pkg-config nasm patchelf autoconf automake libtool libltdl-dev autoconf-archive python3-jinja2 python3-venv libxtst-dev libxxf86vm-dev bison flex gperf libssl-dev libasound2-dev libudev-dev libx11-dev libx11-xcb-dev libxcb1-dev libxrandr-dev libxi-dev libxcursor-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan-dev mesa-vulkan-drivers xvfb xauth unzip jq
sudo apt-get install -y --no-install-recommends libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libglib2.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav libgtk-3-dev libwebkit2gtk-4.1-dev
```

`libltdl-dev` は vcpkg の `libxcrypt` autoreconf ステップが使う
`LT_SYS_SYMBOL_USCORE` マクロを提供します。kirikiroid2-web のネイティブ依存関係ジェネレーターには `python3-jinja2`、Linux GTK 依存閉包には `libxtst-dev` も必要です。これらはビルドホストの前提条件であり、`cargo xtask deps` はシステムパッケージをインストールしません。
`python3-venv` は libsystemd の隔離ビルド環境に ensurepip を提供し、`libxxf86vm-dev` は Linux Cocos のリンクに必要な Xxf86vm ライブラリを提供します。

```sh
cargo xtask deps --target x86_64-unknown-linux-gnu
cargo xtask deps --target x86_64-unknown-linux-gnu --all-cores
cargo xtask build --target x86_64-unknown-linux-gnu --manager --release
cargo xtask build --target x86_64-unknown-linux-gnu --all-cores --release
cargo xtask package --target x86_64-unknown-linux-gnu --manager --version 0.1.0-beta.1 --out .work/linux-manager
cargo xtask package --target x86_64-unknown-linux-gnu --core siglus_rs --out .work/linux-siglus
cargo xtask ci --target x86_64-unknown-linux-gnu --component manager --report .work/linux-ci.json
```

Linux のネイティブコアを個別にビルドする場合、`CORE` を `renpy`、`impacto`、または `kirikiroid2-web` に置き換えてください。

```sh
cargo xtask deps --target x86_64-unknown-linux-gnu --core CORE
cargo xtask ci --target x86_64-unknown-linux-gnu --component CORE --report .work/linux-core-ci.json
```

Ren'Py のパッケージには SDK ランタイムとインストール済みの `Resources` が含まれます。Impacto と KiriKiri2 のパッケージには、固定されたネイティブ共有ライブラリの依存閉包が含まれます。各パッケージの `build.json` はシステムの SONAME 要件を記録します。

Linux の Manager 向け `deps` は、ディストリビューションが提供する開発ライブラリを確認するだけで、`apt` は呼び出しません。コア向けの `deps` は、ターゲットごとに独立したディレクトリでロックされた vcpkg のネイティブライブラリを準備し、パッケージング用に再配置可能な共有ライブラリの通常ファイルを生成します。パッケージのディレクトリで `./shinki-emu-manager` を実行してください。パッケージングでは ELF のアーキテクチャ、ローダーのパス、依存関係の閉包を確認し、ライセンス表記とオフラインのメタデータスナップショットを同梱します。`ci` が報告する ZIP では実行権限が保持されます。CI とリリースのジョブは、同じ依存関係と xtask コマンドを使います。コアが利用できるかどうかは引き続き `matrix` に従い、blocked と宣言されたコアにはバイナリの添付ファイルが作られません。
Linux パッケージは、ディストリビューションが提供する glibc、libstdc++、OpenSSL 3、fontconfig、udev、ALSA の ABI ライブラリに加え、ストアの Web サインイン用に GTK 3 と WebKitGTK 4.1 を使います。導入先のシステムには対応するランタイムパッケージ（Ubuntu では GTK 3 も一緒に入る `libwebkit2gtk-4.1-0`）をインストールし、グラフィックドライバーを備えた X11 または Wayland のデスクトップを用意してください。
Impacto の Linux VA-API ライブラリは、システムのグラフィックススタックに含まれる `libdrm.so.2`（Ubuntu パッケージ `libdrm2`）も必要とし、コアには同梱しません。

RFVP は、ネイティブの動画デコードのために、ディストリビューションが提供する GStreamer 1.x（1.14 以上）、GLib 2.x（2.56 以上）、および base、good、libav プラグインも必要とします。これらはシステムの実行要件として宣言され、コアには同梱されません。Ubuntu では `libgstreamer1.0-0 libgstreamer-plugins-base1.0-0 libglib2.0-0 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav` をインストールします。パッケージ内の `build.json` の `system_dependencies` には、実際に依存するシステムの SONAME が記録されます。

ダウンロードが不安定な場合は、固定バージョンのソースとツールのアーカイブを `.work/vcpkg-downloads` に事前に配置できます。転送には再開可能な分割転送や `rsync --partial` を使ってください。vcpkg は、実際に選ばれたバージョン付き port に基づいて、各アーカイブの SHA512 を引き続き検証します。ディストリビューションの libass/FFmpeg で、ロックされたネイティブビルドを置き換えないでください。

`deps` は既定で、変更のある fork のワークツリーを拒否します。未コミットの adapter の変更をデバッグする場合は、`--allow-dirty-sources` を明示的に指定できます。その場合でもワークツリーの HEAD はインベントリで固定された revision と一致している必要があり、ソースの準備でそのワークツリーに fetch、checkout、サブモジュールの更新は行われません。ビルド記録にはソースの変更状態が残り、`release` は変更のあるソースとそのビルド記録を拒否します。

Linux の CI とリリースのジョブでは、報告された Manager の ZIP を展開し、独立したユーザーデータディレクトリを使って Xvfb 上で起動します。プロセスは 15 秒間動作し続ける必要があり、それより早く終了するとジョブは失敗します。

### Android Manager テスト APK

JDK 27（`JAVA_HOME` を設定し、その `bin` を `PATH` に追加）、Android SDK platform 37.2、build-tools 37.0.0、NDK 30.0.16248370、cargo-ndk 4.1.2 を使います。`ANDROID_HOME` は SDK のルート、`ANDROID_NDK_HOME` はその中の `ndk/30.0.16248370` を指します。固定された安定版 NDK revision のみを受け付けます。SDK パッケージが足りない場合は、その SDK の `sdkmanager --sdk_root=<sdk> --channel=0 "platforms;android-37.2" "build-tools;37.0.0" "ndk;30.0.16248370"` を明示的に実行します。`CARGO_HOME`、`CARGO_TARGET_DIR`、`TEMP`/`TMP`、`GRADLE_USER_HOME` は空き容量の十分なドライブに置いてください。

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo xtask deps --target aarch64-linux-android
cargo xtask deps --target x86_64-linux-android
cargo xtask build --target aarch64-linux-android --manager --release
cargo xtask build --target x86_64-linux-android --manager --release
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-beta.2 --out .work/android-test
```

各ビルドは `cargo ndk-env` のツールチェーン環境を取り込み、Cargo を `--locked` で直接実行します。`static_manager_targets` にそのトリプルを含むコア（RFVP、Siglus、Shinki Musica、Artemis、Impacto、Ren’Py）を静的リンクし、ビルド記録が指す target ディレクトリの `android-inputs/` に、JNI ランタイムの依存関係、証明書検証用の AAR、メタデータ、ライセンスを集めます。KiriKiri は含まれません。パッケージングには、**両方のトリプル**について同じソースコミットから作られた release のビルド記録が必要です。続いて Gradle が `arm64-v8a`、`x86_64`、`assets/metadata/` を含むユニバーサル APK を生成し、xtask が署名と各ネイティブライブラリの依存関係の閉包を確認します。署名鍵がない場合の出力は `ShinkiEMU-<version>-android-universal-debug-signed.apk` です。これは debug 署名の CI テスト用パッケージで、正式なインストーラーではありません。テスト用の鍵は `GRADLE_USER_HOME` 内に生成されます。

Gradle の依存関係を一度ダウンロードしたあとは、`SHINKI_EMU_GRADLE_OFFLINE=1` を設定すると `--offline` が渡されます。パッケージングのたびに新しい出力ディレクトリを使ってください。`cargo xtask ci --target aarch64-linux-android --component manager --report .work/android-ci.json` は両方の ABI をビルドし、実際の APK のパスを報告します。CI は両方のトリプルの依存関係を準備したうえで、このコマンドを 1 回だけ実行します。`matrix --json --ci` が、同じユニバーサル APK を作るだけの `x86_64-linux-android` の Manager 行を除くためです。Release はユニバーサルな release 署名 APK を 1 つだけアップロードし、対応する sources ZIP も公開します。

すべての Manager パッケージは `Assets/Branding/shinkiemu-icon.svg` を共通のアイコンソースとして使います。Rust が Windows EXE の ICO リソース、macOS ICNS、iOS の app icon set、Android のアダプティブランチャー mipmap、Linux の hicolor PNG を生成します。Linux ZIP には `share/applications/shinkiemu.desktop` も含まれます。このファイルと `share/icons/hicolor` をアプリケーションとアイコンのディレクトリにインストールしてください。ゲームがウィンドウアイコンを変更した場合、ゲーム終了時に Manager アイコンへ戻ります。

実行時の制約：標準の Slint 1.18.1 では、SwiftShader やソフトウェア Vulkan のエミュレーター上で最初のフレームしか描画されず、その後の画面が更新されない問題があります。`-gpu host` または実機を使ってください。単一の Text と単一の Timer だけを持つ別アプリでも、5 回のコールドスタートすべてで更新の停止が再現しました。同じ Manager テスト APK は、ホスト GPU モードでの 5 回のコールドスタートすべてで、中国語のオンボーディングとアイコンを正常に表示しました。ビルドやパッケージングの成功は、ゲームの互換性が検証されたことを意味しません。

### Apple Manager：ネイティブ macOS と Linux からのクロスビルド

ネイティブ CI は引き続き `build/targets.toml` の macOS runner を使い、Cargo/Xcode でビルドします。Linux では、`build` と `check` が cross 0.2.5 と `Cross.toml` で設定されたプライベートイメージを自動的に選びます。Apple のターゲットは、`x86_64-apple-darwin`、`aarch64-apple-darwin`、実機向けの `aarch64-apple-ios`、シミュレーター向けの `aarch64-apple-ios-sim` と `x86_64-apple-ios` です。最低システムバージョンは macOS 13.0、iOS 16.0 です。

`build/targets.toml` のターゲットに空でない `blocked` を書くと、そのターゲット上のすべてのコンポーネントがその理由でブロックされ、コアの `supported_targets` と `manager_supported` より優先されます。ジョブマトリクスにはその行が含まれず、`deps`、`build`、`package` は実行を拒否し、リリースノートにはターゲットと理由が成果物なしで載ります。現在は 5 つの Apple ターゲットにこの理由が設定されており、クロスビルドのツールチェーンが検証されるまで CI とリリースから外れます。フィールドを削除すれば元に戻ります。

Xcode は [Mac App Store](https://apps.apple.com/app/xcode/id497799835) または [Apple Developer Downloads](https://developer.apple.com/download/all/) から入手し、Apple のライセンスに従ってください。Mac 上で `xip --expand` を使って公式の Xcode `.xip` を展開し、その Xcode を選択して、同じ Mac 上でライセンスに同意します。Command Line Tools だけでは iOS SDK は提供されません。続いて次を実行します。

```sh
for sdk in macosx iphoneos iphonesimulator; do
  tar -C "$(xcrun --sdk "$sdk" --show-sdk-path)" -cJf "$sdk.sdk.tar.xz" .
done
```

アーカイブは非公開の手段で転送し、Linux 上でそれぞれ別のディレクトリに展開します。`SHINKI_APPLE_SDK_DIR` は、`SDKSettings.json`、`System/`、`usr/` を含む SDK の内容ディレクトリを直接指す必要があり、親ディレクトリやアーカイブを指定してはいけません。Darwin には `macosx`、実機には `iphoneos`、2 つのシミュレーターには `iphonesimulator` を使います。パイプラインがミラーサイトから SDK をダウンロードすることはありません。SDK と SDK を含むイメージを Git や公開レジストリに入れてはいけません。

ソース、`CARGO_HOME`、`RUSTUP_HOME`、`CARGO_TARGET_DIR`、ツールは、`~/shinkirou-apple/` のような専用ディレクトリにまとめて置きます。Rust 1.98.0 と使用する Rust target を明示的にインストールし、`cargo +1.98.0 install --locked --root "$HOME/shinkirou-apple/tools"` で cross 0.2.5 と apple-codesign 0.29.0（`rcodesign` を提供）をインストールして、ツールディレクトリの `bin` を `PATH` に追加します。イメージの作成には `/` に 8 GiB 以上の空き容量が必要で、ユーザーのファイルやイメージを削除することはありません。本リポジトリで次を実行します。

Docker には Buildx/BuildKit が必要です（`docker buildx version` で確認できます）。プライベートなプラグインは `$DOCKER_CONFIG/cli-plugins/docker-buildx` に置けるため、sudo は不要で、ユーザーの既存の Docker 設定も変更しません。[Buildx v0.38.1 の公式 Linux x86_64 用ファイル](https://github.com/docker/buildx/releases/download/v0.38.1/buildx-v0.38.1.linux-amd64)の SHA256 は `e25d5cd675092bb5d1d03632fdc46cdf3fede7b514dc5f023a105e00d3ce5ad4` です。ダイジェストを確認してから実行権限を付与してください。

```sh
export DOCKER_CONFIG="$HOME/shinkirou-apple/docker"
export SHINKI_APPLE_SDK_DIR="$HOME/shinkirou-apple/sdks/macosx.sdk"
cargo xtask apple-image --target aarch64-apple-darwin
cargo xtask deps --target aarch64-apple-darwin
cargo xtask build --target aarch64-apple-darwin --manager --release
cargo xtask package --target aarch64-apple-darwin --manager --version 0.1.0-beta.1 --out .work/macos-manager
export SHINKI_APPLE_SDK_DIR="$HOME/shinkirou-apple/sdks/iphonesimulator.sdk"
cargo xtask apple-image --target aarch64-apple-ios-sim
cargo xtask build --target aarch64-apple-ios-sim --manager --release
cargo xtask package --target aarch64-apple-ios-sim --manager --version 0.1.0-beta.1 --out .work/ios-simulator-manager
```

イメージはターゲットごとに個別に準備し、対応する SDK を選びます。イメージは Rust、osxcross、vcpkg の入力を固定し、LLVM/LLD、C/C++、CMake、Apple 用 triplet を提供します。ホストのコンパイラーとターゲットのコンパイラーは別々に設定されます。ネイティブ依存関係のライセンスは `.work/apple-native/<target>` にエクスポートされ、イメージの識別子が同じ場合に限り再利用されます。SDK、イメージ、Rust target が不足していると `doctor` がエラーを返します。対応ターゲットで実際にコンパイルエラーが起きた場合は、そのまま失敗として扱われます。

macOS のパッケージには `ShinkiEMU.app` が含まれ、実行ファイルは `Contents/MacOS`、メタデータとライセンスは `Contents/Resources` に置かれます。Apple バンドルには ShinkiEMU アプリケーションアイコンも含まれます。`ci`/`release` は ZIP を生成します。iOS の Manager はメインスレッドで Slint の winit/UIKit バックエンドと Metal レンダラーを使います。ネイティブの Files ピッカーはセキュリティスコープ付きの許可とブックマークを保持し、アプリケーションデータはアプリ自身の Library サンドボックスに保存されます。iOS での再起動は、ユーザーがアプリを閉じて開き直す必要があります。共有プラットフォーム層は実際の AVAudioSession を有効にします。Manager がコンパイルできても、iOS で任意にダウンロードしたコアのバイナリをインストールできるわけではありません。コアごとの組み込みとビルドの制約は引き続きインベントリに記載されています。

シミュレーター向けパッケージには `simulator-test.app` が含まれます。実機向けパッケージでは `Payload/*.app` を含む `device-test.ipa` が生成され、`ci` は IPA の実際のパスを報告します。パッケージングでは、単一アーキテクチャの Mach-O、実機/シミュレーターのプラットフォーム、システムと同梱の依存関係の閉包を確認してから、コピーしたファイルに署名します。Linux では `rcodesign sign` を使い、各 Mach-O を個別に検証します。ネイティブの macOS では `codesign` を使います。ad-hoc 署名は実際の署名ですが、Apple の provisioning、公証、App Store の識別情報は提供しません。実機へのインストールには有効な Apple の署名と provisioning が必要です。これらの成果物はテストと統合用で、正式なインストーラーではありません。同じアーキテクチャの添付ファイル名が重複しないよう、Release 名には iOS の実機/シミュレーターのバリアントが含まれます。

SDK を使った受け入れ確認では、実際の成果物のパスとサイズ、ネイティブアーキテクチャと依存関係の確認、各プラットフォームの通常の署名検証結果を保持してください。ネイティブ macOS のパッケージングでは Apple の `codesign` を使います。正規の SDK がない場合、この受け入れ確認は行えません。コンパイルの成功だけでは、ゲームの実行互換性は示されません。

Apple 版 Manager の `build.json` は、バンドルの書き換えと署名より前のネイティブ入力を記録するため、署名後の実行ファイルとはバイトハッシュが異なる場合があります。Release の記録は実際に配布するパッケージを識別します。通常のプラットフォーム署名検証と製品・対象・版の対応確認を維持し、入力の記録を署名後バイナリのハッシュに付け替えないでください。

Shinki Musica は iOS Manager の静的レジストリとコア一覧に含まれ、FFmpeg の依存関係はターゲットごとに分離されます。独立したコミット `72ac926e1d15a452dc81ca84f3e82776aa302a26` は、実機ターゲットと両シミュレーターターゲットの exact-head CI、ネイティブコンパイル、パッケージ、署名、依存関係、ライセンスの確認を通過しました。ネイティブセーブと統合した後の Manager リリースには、新しい統合コミットでの確認が必要です。iOS では動的コアの単独インストールをサポートしません。デバイスでの起動確認は免除されています。

FFmpeg を組み込む Manager には、対象の専用インストールツリーから FFmpeg と推移的依存 port のライセンスを `licenses/ffmpeg/` に同梱します。単体コアのパッケージも同じ収集処理を使います。

ネイティブ静的コアは Cargo のビルドマニフェストに `native-static` アーカイブと実行時リソースを記録します。Manager のビルド記録は依存コアのマニフェストを保持し、パッケージではリソースを `cores/<id>/Resources` に配置します。依存関係の準備は組み込むコアごとに行い、`SHINKI_EMU_NATIVE_VCPKG_ROOT` に個別のインストールツリーを置きます。Apple の静的リンクではネイティブのアーカイブを結合し、対象 port のライブラリとシステム framework を Cargo に渡します。

ネイティブ vcpkg のインストール先は `.work/native-vcpkg/<core>/installed-<triplet>/<triplet>` です。別の ABI の依存関係が削除されないよう、ビルド補助ライブラリと xtask が同じパスマッピングを使います。Android の静的リンクにはコア内部のネイティブライブラリ、ターゲットの vcpkg アーカイブ、pkg-config の private 依存関係を含め、APK には NDK の C++ 共有ランタイムを同梱します。Windows ホストでは vcpkg のビルドロックを共有するため、作業ツリーをまたぐ依存関係のインストールも直列に実行してください。

## OpenHarmony Manager

ローカルビルドには DevEco Studio 付属の OpenHarmony Native SDK 5.0.5（API 17。HAP は API 12 以降が対象）と Rust 1.99.1 が必要です。`OHOS_SDK_NATIVE` を `sdk/default/openharmony/native` に設定すると、xtask は同じ DevEco インストールから Node、hvigor、ohpm、CMake/Ninja、`hap-sign-tool.jar`、`llvm-readelf` を見つけます。`SHINKI_EMU_OHOS_SIGNING_DIR` は checkout 外のディレクトリ（例：`.shinki-secrets/ohos-debug`）に設定します。

```sh
cargo xtask deps --target aarch64-unknown-linux-ohos
cargo xtask build --target aarch64-unknown-linux-ohos --manager --release
cargo xtask package --target aarch64-unknown-linux-ohos --manager --version <version> --out <dir>
```

`deps` は `arm64-ohos` triplet で libass と FFmpeg をビルドします。Meson 1.12 の prefix には、Android の triplet と同じく Windows の拡張パスを使います。ビルドは SDK の clang でリンクし、`libc++_shared.so` を同梱します。`package` は Manager のライブラリを `Platforms/OHOS/entry/libs/arm64-v8a` に、メタデータのスナップショット、通知、ライセンスを `rawfile/bundled` に配置し、`ohpm install` と `hvigorw assembleHap` を実行します。初回は SDK の OpenHarmony テスト PKI から debug 署名データを作ります。`OpenHarmony.p12` のコピーに新しい鍵を作り、テスト用アプリ CA でアプリ証明書を発行し、profile-debug 鍵で `org.shinkirou.shinkiemu` の debug profile に署名します。`sign-app` で署名した後、`verify-app` で検証し、アーカイブの内容と、同梱した各ライブラリの `llvm-readelf -d` を出力します。残る `NEEDED` はすべて OHOS のシステムライブラリでなければなりません。出力先には `ShinkiEMU-<version>-ohos-arm64-v8a-debug-test-signed.hap`、`ohos.json`、`build.json` が入ります。keystore、証明書、profile は Git にコミットしません。

テスト PKI で署名した HAP は実験用のテスト配布です。署名、内容、同梱ライブラリの依存関係を確認します。デバイス・エミュレーターでの起動確認は免除されており、市販デバイスへのインストール可能性は未確認です。インストール確認には DevEco/AppGallery Connect が発行した profile が必要です。

## マトリクスと CI

`matrix` はすべての Manager/コアの組み合わせと blocked 理由を表示します。`matrix --json` は GitHub runner のある行のみをジョブとして選びます。OHOS Manager は実験用のテスト署名配布に対応しますが `runner = "local-only"` であり、DevEco でビルドした成果物をローカルから引き渡します。`--ci` は重複する Android x86_64 Manager 行も除きます。現在の対応状況は[コア一覧](https://project-shinkirou.github.io/ja/shinkiemu/cores/)を参照してください。

```sh
cargo xtask ci --target x86_64-pc-windows-msvc --component rfvp --report .work/reports/rfvp-windows.json
```

未対応の組み合わせは `blocked`、`artifact: null` を報告します。OHOS Manager のローカル署名ビルドは、同じコミットの成功したリモート CI と結合してから公開します。対応対象のビルド失敗は `failed` を返します。`publish_blocked` は配布上の制限を別に記録します。

GitHub Actions が正式な CI であり、ホストされたプラットフォームのリリース成果物をビルドします。DevEco が必要な OHOS HAP は、以下のゲート付きローカル引き渡しで追加します。
OHOS Manager を含むリリースでは、release ブランチを push し、同じコミットの `ci.yml` の成功を確認します。`components=manager` で `release.yml` を実行します。このワークフローは全ホスト行の成功を要求し、Manager の公開はローカル引き渡しに残します。実行中の `release-files-manager@*` artifact をすべてダウンロードし、その `<tag>/<target>/` を同じコミットのクリーンな checkout の `.work/release/<tag>/` に配置します。DevEco と checkout 外の署名ディレクトリを設定して、次を実行します。

```sh
cargo xtask deps --target aarch64-unknown-linux-ohos
cargo xtask build --target aarch64-unknown-linux-ohos --manager --release
cargo xtask release --component manager --target aarch64-unknown-linux-ohos --version <version>
SHINKI_EMU_RELEASE_CI_RUN_ID=<成功した-ci-実行-id> SHINKI_EMU_RELEASE_HANDOFF_RUN_ID=<成功した-handoff-実行-id> GH_TOKEN=<GitHub-token> cargo xtask release --component manager --version <version> --publish
```

最後の公開コマンドの前に、同じコミットで `manager-v<version>` のドラフトを作成し、OHOS HAP、対応ソース ZIP、`ohos-handoff.json` という名前にした OHOS の `release.json` をアップロードします。同じコミットの `release.yml` に `components=manager`、バージョン、`verify_local_manager=true` を指定します。この検証専用実行は実際のドラフト成果物を取得し、SHA256、HAP の内容、ネイティブ依存関係と対応ソースのコミットを確認します。ホスト runner に DevEco は不要です。検証済みファイルは `verified-ohos-handoff` artifact に保存されます。成功した実行 ID を `SHINKI_EMU_RELEASE_HANDOFF_RUN_ID` に設定します。テスト PKI 署名はローカル DevEco で確認し、ホスト側は成果物の完全性を確認します。ホスト側での再署名検証やデバイス検証は行いません。

公開処理は同一コミット、全 archive の SHA256、全ホスト行（重複する Android x86_64 行を除く）と OHOS HAP、および同一コミットの両ワークフロー成功を照合します。公開名と説明にはテスト署名と起動検証の免除を明記します。

ソースバンドルの字幕ライブラリ再ビルドコマンドは `build/vcpkg/mobile-triplets` を含み、OHOS の `arm64-ohos` triplet と同梱のネイティブソースアーカイブを使用します。
検証専用ジョブは未公開のドラフト成果物を取得するためにリポジトリの `contents: write` 権限を必要としますが、ドラフトの公開や変更は行いません。

ローカルランナーは push 前の任意の事前チェックで、リモートで失敗する問題を先に見つけるためのものです。`matrix --json [--ci] --since <ref>` は、入力が `<ref>` 以降に変わった行だけを残します。ランナーは `SHINKI_CI_SINCE` からこの引数を渡します。共有の入力はすべての行を選びます。対象はルートの `Cargo.toml`/`Cargo.lock`、`rust-toolchain.toml`、`xtask/`、`Source/{SDK,FamilyApi,Tools}/`、`build/targets.toml`、`build/vcpkg/`、`build/ci/`、およびここに挙げていないすべてのパスです。`Source/Manager/` と `Source/Providers/` は Manager の行を選びます。`Source/Families/<ディレクトリ>/` は、そのディレクトリにマニフェストがあるコアと、そのコアの `static_manager_targets` 上の Manager の行を選びます。`build/cores.toml` の `[[cores]]` テーブルが変わった場合も、そのコアについて同じです。`docs/**`、`Metadata/**`、Markdown ファイルはどの行も選びません。

## パッケージングとリリース

まず対象のコンポーネントをビルドし、次にビルド記録が指す成果物をパッケージングします。

```sh
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --out .work/packages/rfvp
cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-beta.1 --out .work/packages/manager
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc
```

出力ディレクトリはまだ存在しないものでなければなりません。パッケージングは、安全でないパス、リンク、名前の衝突、ライセンスの欠落、アーキテクチャの誤り、query エクスポートの欠落、ネイティブ依存関係の不足を含む入力を拒否します。コアパッケージには、ライブラリ、リソース、依存ファイル、ライセンス、ビルドの来歴、ハッシュ付きの `core.json` が含まれます。ソースパッケージには、該当する対応ソースと再リンク用の入力が含まれます。

`shinki-emu-<slug>-<バージョン>-<プラットフォーム>-sources.zip` には `source.tar` と `BUILD-FROM-SOURCE.md` が入っています。TAR には、リリースコミット時点の製品のコミット済みツリーと、選択した fork の固定 revision のコミット済みツリーが含まれ、どちらもサブモジュールを記録された revision で展開します。ワークツリー上の未コミットの変更は含まれません。Git LFS のポインターは、コミットされた SHA256 とサイズを確認したうえで実体のデータに置き換えます。データがない場合は先に `git lfs checkout` を実行してください。

Rust の依存関係は変更していない `Cargo.lock`（registry のチェックサムと正確な Git コミット）で特定され、再ビルド時に Cargo が取得します。さらに `rust-sources/<名前>-<バージョン>+<ソース>/` には、ライセンスがソースの提供を求めるリンク済み crate のソースを入れます。対象は、選べるライセンスがすべて copyleft の crate（MPL-2.0 の symphonia、triple_buffer、option-ext など）と SPDX 式のない crate です。GPL/LGPL/AGPL でしか使えない crate が 1 つでもリンクされている場合（ReMinori）は、リンク済みの crate をすべて含めます。寛容なライセンスを選べる crate（`Apache-2.0 OR GPL-2.0-only`、Slint の `GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR …`）はその選択肢で配布するため、ソースの提供義務は生じません。パス crate は fork のスナップショットに含まれます。

ネイティブのソースは、選択したインストールツリーの vcpkg port をすべて対象にし、ホスト triplet のツールも含めて `vcpkg/status` から依存関係をたどります。`native/ports/<グループ>/` には、インストール時の SPDX `gitoid` が示す固定 registry tree から取り出した各 registry port の recipe とパッチを置きます。overlay port には tree の識別子がなく、fork のスナップショットに含まれます。`.work/vcpkg-downloads/` には、インストール済み SPDX が SHA512 で記録したソースアーカイブを置きます。ビルド手順では固定 revision の vcpkg を clone します。Impacto では準備済みの LibAtrac9/magic_enum のソースも含め、mspack は fork のスナップショットに含まれます。

ソースパッケージは、`deps` と `build` が確認済みの入力をそのままコピーします。`.work/downloads` のハッシュの再計算、recipe の再検証、Cargo のオフライン解決、ワークツリーの再確認は行いません。`release` 自体は、未コミットの変更がある製品の checkout と、別のソース revision から作られたビルド記録を拒否します。

`cargo xtask deps --core renpy --target x86_64-pc-windows-msvc` は、固定された `.work/renpy-build` の checkout と再帰的な submodule を準備します。既存の checkout に未コミットの変更がある場合は更新前に失敗し、ソース収集は読み取り専用のままです。

Manager のパッケージングでは、ターゲットプラットフォームで絞り込んだ Cargo の依存関係の閉包（通常の依存とビルド依存。テスト専用の依存エッジは除く）を収集します。各 crate が宣言した `license_file`、ソースルートの `LICENSE*`、`COPYING*`、`NOTICE*` ファイル、および同種のライセンスディレクトリの内容は `licenses/rust/<名前>-<バージョン>/` に置かれ、同じディレクトリの `package.json` に SPDX 式とテキストの出所が記録されます。crate がファイルを提供していない場合、パッケージャーは固定された `spdx` 0.13.6 に内蔵されたオフラインのデータから SPDX License List 3.29.0 の標準テキストを取り出し、SPDX 式が参照するすべてのライセンスと例外条項を `LICENSE-<SPDX>` として書き出します。データの出所となるバージョンとアーカイブの SHA256 は `build/dependencies.toml` に記録され、Cargo の固定依存と照合されます。パッケージング中にライセンステキストをダウンロードすることはありません。認識できない補完用の識別子や、ライセンスファイルも SPDX 式もない crate があると、パッケージングは失敗します。Material コンポーネント、アイコン、ネイティブ依存関係のライセンスは別途収集されます。

Manager と自作 CMVS/Musica は独立した SemVer および `manager-v<semver>`、`core-<slug>-v<semver>` タグを維持し、`--version` を指定します。RFVP、Artemis を含む外部コアは `build/cores.toml` から識別子を計算するため `--version` は省略します。`cargo xtask release --component <id> --dry-run` は元の upstream ref、完全なコミットと adapter pin の関係だけを検証し、ビルド・公開しません。`release --target <トリプル>` は `.work/release/<tag>/<target>/` にバイナリ ZIP、対応ソース ZIP と `release.json` を作成します。ブロック対象と `publish_blocked` コアは成果物を作りません。

外部識別子は原作者の `upstream {repo, ref_kind, ref_name, commit}` と組織の `adapter {repo, branch, commit}` を別々に保持します。コミットは小文字 40 桁の完全なハッシュです。upstream の該当コミットに検証済みタグがあればタグを使い、なければ本来の upstream ブランチを使います。組織の統合ブランチを upstream と表記しません。表示は `<ref>@<upstream commit12> (adapter <adapter commit12>)` です。ファイル用の版は `<tag|branch>-<ref slug>-<digest>`。ref の先頭 16 ASCII バイトを取り、英数字を小文字化し、それ以外をそれぞれ `-` にします。digest は upstream repo/kind/ref/commit、adapter repo/branch/commit、十進数の `release_sequence` の順で、各 UTF-8 フィールドに LF を付けた列の完全な SHA256 です。版は最大 88 バイト、タグは `core-<plugin slug>-<version>` です。plugin slug は ID を小文字化して `.` と `_` を `-` に置換します。ブランチの `/` はパスに入らず、adapter のみの変更でも別の識別子になります。

外部 plugin slug は最大 32 バイトで、Release タグは安全な ASCII 文字で最大 126 バイトです。

`release_sequence` は外部コアごとの厳密に増加する正整数（最大 9007199254740991）です。upstream、adapter pin の変更や意図した再ビルドで増加させます。タグ版とブランチ版は同じ連番で最新を選び、ブランチ名やハッシュは大小比較しません。型付き識別子は旧 beta エントリより優先し、残りは SemVer 順です。異なる版で同じ連番を使うことは、別プラットフォームでも拒否します。パッケージ、埋め込みコア一覧、ソース参照とカタログは同じ来歴を保持し、元のライセンス・著作権表示と対応ソースの要件も維持します。

[Impacto 連番3のリリース](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-impacto-tag-0-10-8-930-96de7434c8b1a57df7ff24dcee829282194873fe596677056fa168485cb1555e) は、製品コミット `5b197c1c656186d3a535bb3486311623516166e5`、upstream `0.10.8.930`、adapter `34b89c29123d025d96f73ac9a746fb388e572065` から Windows、Linux、両 macOS アーキテクチャのパッケージを公開しています。4対象の対応ソース・ネイティブ入力・ライセンス・パッケージ監査と、認証なしでの全8バイナリ/ソース添付ファイルの実ダウンロード・ハッシュ照合が完了しました。この単独版は Manager6 内蔵の `910a820ba3ea8d4dd8d6e19c560df1d5a3bc5157`/連番2を置き換えず、公開済み連番1の資産も変更しません。カタログは公開完了の順番ではなく連番3を選びます。

[Ren’Py 連番1のリリース](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-renpy-tag-8-5-3-26051504-5f537560e1a1b852070dec65946d6c027e9f4f7482bddd8349ed47960c383133) は、製品コミット `05326439dcfafb81df79f3dabe0997fb696ad3f3`、upstream `8.5.3.26051504`、adapter `a52d9d9c006223c2b5a276c05e1b97ffeecbe66f` から4デスクトップ対象を公開しています。全対応ソースパッケージで、107バイトのシンボリックリンク先を含む固定再ビルド用レシピ389/389項目の実物確認が完了しました。公開された全8バイナリ/ソース添付ファイルを認証なしで実際に取得し、受け入れ済み記録とハッシュを照合しました。元のネイティブライセンス本文も固定 fork と原入力に対してバイト単位で確認済みです。

同じ index に `cores-v2.json`（`shinki.emu.cores.v2`）を追加し、旧エントリと新識別子を収録します。`cores.json` は v1 と identity のない公開済みエントリを維持するため、旧 Manager のダウンロードは継続します。新 Manager は index にある v2 を優先し、旧 v1 snapshot も受け入れます。公開済み beta のタグ、添付ファイルや来歴は変更せず、引き続きインストールできます。更新は adapter crate の版ではなくパッケージの識別子を比較します。

upstream 識別子付きパッケージには Manager `0.1.0-beta.6` 以降が必要です。beta.5 以前は追加の識別子と長い版フィールドを拒否するため、旧カタログと旧パッケージを維持してください。外部コアの新規 Release にはこの要件を明記します。Sakura recipe 0.2 は、対応 Manager と更新版 RFVP の両方を公開入手できるようになってから公開します。

`cargo xtask release --component <id> --version <semver> --publish`（`GH_TOKEN` または `GITHUB_TOKEN` が必要）は、その tag で準備済みのターゲットをすべて集めます。別の製品コミットや別のコア固定 revision の記録、別のコミットを指す既存の tag、別のコミットから作られた draft、空の Release、公開済みの Release は拒否します。実行中のコミットで draft を作るか、そのコミットの draft を再利用し（中断した実行が残した同名の添付ファイルは置き換えます）、ソースとバイナリのアーカイブをアップロードして公開します。tag がなければ GitHub がそのコミットに作成します。コアの場合は続けて、各プラットフォームのカタログエントリを `metadata/release-<tag>` ブランチの `Metadata/engines/cores/` にコミットし、`main` への PR を作ります。組織の設定で Actions による PR 作成が許可されていない場合、この最後の手順は失敗しますが、エントリはすべてブランチにあるので、PR を手動で作成してください。カタログ URL は最終 tag から作り、draft の `untagged-*` URL は使いません。Manager とコアは別々にリリースされます。Android の debug 署名テスト用 APK はブロック扱いとなり、アップロードされません。

`release.yml` は選択したコンポーネントのホスト対象行をすべてビルドし、ローカル専用ターゲットのないコンポーネントを公開します。OHOS 対応 Manager は、上記のゲート付きローカル引き渡しで HAP とソースを揃えてから公開します。単一のコアは tag を push してリリースできます。複数のコンポーネントは `version` と `components` を指定してワークフローを実行します。新しい tag を一度に 4 件以上 push してもワークフローは起動しません。

公開せず準備する場合も同じ `release.yml` を使い、`prepare_only=true` と、必要に応じて正確な40桁の製品コミット `source_commit` を指定します。明示したソースコミットは準備専用モードでのみ使えます。既存のネイティブビルド、パッケージ、プラットフォーム署名、ソースとライセンスの同梱処理は変えません。最後の引き渡しでは各記録が実際の製品コミット、コンポーネント、版、対象に一致することを要求し、公開をスキップします。workflow dispatch SHA は編成コードの識別子であり、製品の来歴ではありません。受け入れ済み製品のクリーンな checkout から既存の xtask で実物を公開してください。CI 成功だけでは、成果物の取得可否、通常のパッケージ構造、署名要件を代替できません。

dispatch の `version` は Manager/自作コアだけに適用します。外部コンポーネントはプラットフォーム行を展開する前に各自の識別子を検証・計算します。外部コアを上記の公開コマンドで公開するときは `--version` を省略し、Manager/自作コアでは指定します。

メタデータのコマンドは [Metadata](metadata.md)、コアパッケージの契約は[コア ABI](core-abi.md) を参照してください。
