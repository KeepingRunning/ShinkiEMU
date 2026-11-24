#!/usr/bin/env bash
set -eu
root=$PWD
target=$1
git -C upstream apply "$root/Tools/leaf/upstream.patch"
if [ ! -s media/lib/libavformat.a ]; then bash Tools/leaf-mobile/media-build.sh "$target"; fi
args=(-G Ninja -DCMAKE_BUILD_TYPE=Release -DUPSTREAM="$root/upstream" -DLEAF_MEDIA_PREFIX="$root/media"
      -DLEAF_BUNDLED_SDL=ON -DLEAF_MOBILE_PLAYER=ON)
mkdir -p out
case "$target" in
  android-*)
    case "$target" in android-arm64) abi=arm64-v8a;; android-x86_64) abi=x86_64;; esac
    args+=(-DCMAKE_TOOLCHAIN_FILE="$ANDROID_NDK_HOME/build/cmake/android.toolchain.cmake"
           -DANDROID_PLATFORM=android-28 -DANDROID_ABI="$abi" -DANDROID_STL=c++_static)
    ;;
  ios-*)
    case "$target" in ios-arm64) sdk=iphoneos; arch=arm64;; ios-sim-arm64) sdk=iphonesimulator; arch=arm64;; ios-sim-x86_64) sdk=iphonesimulator; arch=x86_64;; esac
    args+=(-DCMAKE_PROJECT_INCLUDE="$root/Tools/leaf-mobile/apple-languages.cmake" -DCMAKE_SYSTEM_NAME=iOS -DCMAKE_OSX_SYSROOT="$sdk" -DCMAKE_OSX_ARCHITECTURES="$arch"
           -DCMAKE_OSX_DEPLOYMENT_TARGET=15.0 -DCMAKE_XCODE_ATTRIBUTE_CODE_SIGNING_ALLOWED=NO)
    ;;
esac
cmake -S Tools/leaf -B mobile-build "${args[@]}"
cmake --build mobile-build --target leaf_player -j 2
case "$target" in
  android-*)
    mkdir -p "out/$abi"
    cp mobile-build/libmain.so mobile-build/sdl/libSDL2.so "out/$abi/"
    readelf -h "out/$abi/libmain.so" | grep Machine
    readelf -d "out/$abi/libmain.so" | grep NEEDED
    readelf -Ws "out/$abi/libmain.so" | grep -w SDL_main > mobile-build/entrypoint.txt
    ;;
  ios-*)
    app=mobile-build/Leaf.app
    bash Tools/leaf-mobile/notices.sh "$app/licenses"
    plutil -lint "$app/Info.plist"
    lipo "$app/Leaf" -verify_arch "$arch"
    xcrun vtool -show-build "$app/Leaf"
    if otool -L "$app/Leaf" | grep -Eq '/opt/homebrew|/usr/local|/Users/runner'; then exit 1; fi
    if [[ "$sdk" == iphoneos ]]; then
        mkdir -p Payload
        cp -R "$app" Payload/
        /usr/bin/zip -qr "out/Leaf-$target-unsigned.ipa" Payload
    else
        codesign --force --sign - "$app"
        codesign --verify --strict "$app"
        ditto -c -k --keepParent "$app" "out/Leaf-$target.app.zip"
    fi
    ;;
esac
