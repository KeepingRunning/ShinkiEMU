#!/usr/bin/env bash
set -eu
root=$PWD
native=$OHOS_SDK_NATIVE
git -C upstream apply "$root/Tools/leaf/upstream.patch"
git -C upstream apply "$root/Tools/leaf-ohos/libcxx.patch"
cmake -S Tools/leaf-ohos -B build-ohos -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DUPSTREAM="$root/upstream" -DLEAF_MEDIA_PREFIX="$root/media" -DCMAKE_TOOLCHAIN_FILE="$native/build/cmake/ohos.toolchain.cmake" \
  -DOHOS_ARCH=arm64-v8a -DOHOS_STL=c++_shared
cmake --build build-ohos --target leaf_ohos -j 3 -- -k 0
cp -R Platforms/OHOS leaf-hap
cp Tools/leaf-ohos/Index.ets leaf-hap/entry/src/main/ets/pages/Index.ets
mkdir -p leaf-hap/entry/libs/arm64-v8a
cp build-ohos/libshinki_emu_manager.so leaf-hap/entry/libs/arm64-v8a/
runtime=$(find "$native" -path '*/aarch64-linux-ohos/libc++_shared.so' -print -quit)
test -s "$runtime"
cp "$runtime" leaf-hap/entry/libs/arm64-v8a/
notices=leaf-hap/entry/src/main/resources/rawfile/licenses
bash Tools/leaf-mobile/notices.sh "$notices"
mkdir -p "$notices/ohos-sdk"
cp "$native/NOTICE.txt" "$notices/ohos-sdk/"
cp "$native/llvm/NOTICE" "$notices/ohos-sdk/llvm-NOTICE"
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
python3 Tools/leaf-ohos/sign-test.py out/Leaf-OHOS-arm64-unsigned.hap out/Leaf-OHOS-arm64-debug-test-signed.hap
