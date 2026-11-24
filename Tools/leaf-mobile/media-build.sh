#!/usr/bin/env bash
set -eu
root=$PWD
target=$1
mkdir -p media-build
if [ ! -d ffmpeg-8.0.1 ]; then
  curl -fLS --retry 4 https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz -o ffmpeg-8.0.1.tar.xz
  python3 -c 'import hashlib; assert hashlib.sha256(open("ffmpeg-8.0.1.tar.xz","rb").read()).hexdigest()=="05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41"'
  tar -xf ffmpeg-8.0.1.tar.xz
fi
flags=(--enable-cross-compile --enable-static --disable-shared --disable-asm --disable-symver)
case "$target" in
  android-*)
    tool="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
    case "$target" in android-arm64) cpu=aarch64; triple=aarch64-linux-android;; android-x86_64) cpu=x86_64; triple=x86_64-linux-android;; *) exit 2;; esac
    flags+=(--target-os=android --arch="$cpu" --cc="$tool/${triple}28-clang" --ar="$tool/llvm-ar" --ranlib="$tool/llvm-ranlib" --nm="$tool/llvm-nm" --strip="$tool/llvm-strip")
    ;;
  ios-*)
    case "$target" in
      ios-arm64) sdk=iphoneos; cpu=arm64; triple=arm64-apple-ios15.0;;
      ios-sim-arm64) sdk=iphonesimulator; cpu=arm64; triple=arm64-apple-ios15.0-simulator;;
      ios-sim-x86_64) sdk=iphonesimulator; cpu=x86_64; triple=x86_64-apple-ios15.0-simulator;;
      *) exit 2;;
    esac
    sysroot=$(xcrun --sdk "$sdk" --show-sdk-path)
    flags+=(--target-os=darwin --arch="$cpu" --sysroot="$sysroot" --cc="clang -target $triple -isysroot $sysroot" --host-cc=clang --disable-videotoolbox --disable-audiotoolbox --disable-securetransport)
    ;;
  *) exit 2 ;;
esac
(cd media-build && bash "$root/Tools/leaf/media.sh" "$root/ffmpeg-8.0.1" "$root/media" "$root/upstream/config/media_codecs.json" "${flags[@]}")
