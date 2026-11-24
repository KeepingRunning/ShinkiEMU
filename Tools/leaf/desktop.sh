#!/usr/bin/env bash
set -eu
root=$PWD
git -C upstream apply "$root/Tools/leaf/upstream.patch"
args=()
case "$LEAF_TARGET" in
  linux*) pattern='libshinki_emu_wa2_trial.so' ;;
  windows*|macos*)
    curl -fLS --retry 4 https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz -o ffmpeg-8.0.1.tar.xz
    python3 -c 'import hashlib; assert hashlib.sha256(open("ffmpeg-8.0.1.tar.xz","rb").read()).hexdigest()=="05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41"'
    tar -xf ffmpeg-8.0.1.tar.xz
    mkdir -p ff-build
    flags=()
    case "$LEAF_TARGET" in
      windows*) flags+=(--target-os=mingw32 --arch=x86_64 --disable-pthreads --enable-w32threads --disable-asm); pattern='shinki_emu_wa2_trial.dll';;
      macos*) flags+=(--install-name-dir=@rpath); pattern='libshinki_emu_wa2_trial.dylib';;
    esac
    (cd ff-build && bash "$root/Tools/leaf/media.sh" "$root/ffmpeg-8.0.1" "$root/media" "$root/upstream/config/media_codecs.json" "${flags[@]}")
    export PKG_CONFIG_PATH="$root/media/lib/pkgconfig"
    args+=(-DLEAF_BUNDLED_SDL=ON)
    ;;
esac
cmake -S Tools/leaf -B leaf-build -G Ninja -DCMAKE_BUILD_TYPE=Release -DUPSTREAM="$root/upstream" "${args[@]}"
cmake --build leaf-build --target wa2_trial -j 2
python3 Tools/leaf/package.py "$LEAF_TARGET" "leaf-build/$pattern" leaf-build upstream media out
python3 -m zipfile -e "out/ShinkiEMU-Leaf-$LEAF_TARGET.zip" query-package
case "$LEAF_TARGET" in
  windows*) cc -municode -I Source/FamilyApi/shinki-emu-abi/include Tools/krkr-query-probe.c -o leaf-probe.exe
    ./leaf-probe.exe "$(cygpath -w "$root/query-package/$pattern")" > out/abi.json ;;
  *) cc -I Source/FamilyApi/shinki-emu-abi/include Tools/krkr-query-probe.c -ldl -o leaf-probe
    ./leaf-probe "$root/query-package/$pattern" > out/abi.json ;;
esac
cat out/abi.json
