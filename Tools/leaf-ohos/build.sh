#!/usr/bin/env bash
set -eu
root=$PWD
native=$OHOS_SDK_NATIVE
git -C upstream apply "$root/Tools/leaf/upstream.patch"
curl -fLS --retry 4 https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz -o ffmpeg.tar.xz
echo '05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41  ffmpeg.tar.xz' | sha256sum -c
tar -xf ffmpeg.tar.xz
mkdir -p ffbuild
(cd ffbuild && bash "$root/Tools/leaf/media.sh" "$root/ffmpeg-8.0.1" "$root/media" "$root/upstream/config/media_codecs.json" \
  --enable-static --disable-shared --disable-asm --disable-symver --enable-cross-compile --target-os=linux --arch=aarch64 \
  --sysroot="$native/sysroot" --cc="$native/llvm/bin/clang --target=aarch64-linux-ohos" --ar="$native/llvm/bin/llvm-ar" \
  --ranlib="$native/llvm/bin/llvm-ranlib" --nm="$native/llvm/bin/llvm-nm" --strip="$native/llvm/bin/llvm-strip")
cmake -S Tools/leaf-ohos -B build-ohos -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DUPSTREAM="$root/upstream" -DLEAF_MEDIA_PREFIX="$root/media" -DCMAKE_TOOLCHAIN_FILE="$native/build/cmake/ohos.toolchain.cmake" \
  -DOHOS_ARCH=arm64-v8a -DOHOS_STL=c++_shared
cmake --build build-ohos --target leaf_ohos -j 2
cp -R Platforms/OHOS leaf-hap
cp Tools/leaf-ohos/Index.ets leaf-hap/entry/src/main/ets/pages/Index.ets
mkdir -p leaf-hap/entry/libs/arm64-v8a
cp build-ohos/libshinki_emu_manager.so leaf-hap/entry/libs/arm64-v8a/
runtime=$(find "$native" -path '*/aarch64-linux-ohos/libc++_shared.so' -print -quit)
test -s "$runtime"
cp "$runtime" leaf-hap/entry/libs/arm64-v8a/
bash Tools/leaf-mobile/notices.sh leaf-hap/entry/src/main/resources/rawfile/licenses
python3 - <<'PY'
from pathlib import Path
import json
base=Path("leaf-hap"); p=base/"AppScope/app.json5";d=json.loads(p.read_text());d["app"].update(bundleName="io.github.keepingrunning.leaftrial",vendor="KeepingRunning",versionCode=3,versionName="0.1.3");p.write_text(json.dumps(d))
for p in (base/"AppScope/resources/base/element/string.json",base/"entry/src/main/resources/base/element/string.json"):
 d=json.loads(p.read_text())
 for s in d["string"]:s["value"]="Leaf"
 p.write_text(json.dumps(d))
p=base/"build-profile.json5";p.write_text(p.read_text().replace("5.0.5(17)","5.1.0(18)"))
p=base/"entry/src/main/module.json5";d=json.loads(p.read_text());d["module"].pop("requestPermissions",None);d["module"]["abilities"][0]["orientation"]="landscape";p.write_text(json.dumps(d))
PY
(cd leaf-hap && ohpm install && hvigorw assembleHap --mode module -p product=default -p buildMode=debug --no-daemon)
mkdir -p out
hap=$(find leaf-hap/entry/build -name '*-unsigned.hap' -print -quit)
test -s "$hap"
cp "$hap" out/Leaf-OHOS-arm64-unsigned.hap
unzip -tq out/*.hap
"$native/llvm/bin/llvm-readelf" -d build-ohos/libshinki_emu_manager.so | grep NEEDED
