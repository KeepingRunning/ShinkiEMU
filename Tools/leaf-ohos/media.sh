#!/usr/bin/env bash
set -eu
root=$PWD
native=$OHOS_SDK_NATIVE
curl -fLS --retry 4 https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz -o ffmpeg.tar.xz
echo '05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41  ffmpeg.tar.xz' | sha256sum -c
tar -xf ffmpeg.tar.xz
mkdir -p ffbuild
(cd ffbuild && bash "$root/Tools/leaf/media.sh" "$root/ffmpeg-8.0.1" "$root/media" "$root/upstream/config/media_codecs.json" \
  --enable-static --disable-shared --disable-asm --disable-symver --enable-cross-compile --target-os=linux --arch=aarch64 \
  --sysroot="$native/sysroot" --cc="$native/llvm/bin/clang --target=aarch64-linux-ohos" --ar="$native/llvm/bin/llvm-ar" \
  --ranlib="$native/llvm/bin/llvm-ranlib" --nm="$native/llvm/bin/llvm-nm" --strip="$native/llvm/bin/llvm-strip")
