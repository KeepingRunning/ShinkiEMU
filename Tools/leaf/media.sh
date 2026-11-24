#!/usr/bin/env bash
# Shared, replaceable FFmpeg libraries built from the same source for each target.
set -eu
src=$1
prefix=$2
policy=$3
shift 3
components() {
  python3 -c 'import sys,json; p=json.load(open(sys.argv[1])); print(",".join(x for k in sys.argv[2:] for x in p[k]))' "$policy" "$@"
}
"$src/configure" --prefix="$prefix" --disable-everything --disable-autodetect \
  --disable-network --disable-programs --disable-doc --disable-avdevice --disable-avfilter \
  --disable-debug --enable-small --enable-pic --enable-shared --disable-static \
  --enable-avformat --enable-avcodec --enable-avutil --enable-swscale --enable-swresample \
  --enable-demuxer="$(components demuxers)" --enable-decoder="$(components video_decoders audio_decoders legacy_wave_decoders)" \
  --enable-parser="$(components parsers)" "$@"
make -j "${LEAF_JOBS:-2}"
make install
