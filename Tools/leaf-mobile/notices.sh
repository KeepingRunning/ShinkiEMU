#!/usr/bin/env bash
set -eu
dest=$1
mkdir -p "$dest/upstream" "$dest/SDL2" "$dest/FFmpeg" "$dest/ShinkiEMU"
cp LICENSE-MIT LICENSE-APACHE "$dest/ShinkiEMU/"
cp upstream/LICENSE upstream/NOTICE.md upstream/THIRD_PARTY_NOTICES.md "$dest/upstream/"
for path in third_party/stb/README.md third_party/stb/stb_vorbis.c third_party/wa2_subtitles/LICENSE third_party/wa2_subtitles/FONT-NOTICE.txt; do
  mkdir -p "$dest/upstream/$(dirname "$path")"; cp "upstream/$path" "$dest/upstream/$path"
done
cp upstream/tools/SDL-release-2.32.10/LICENSE.txt "$dest/SDL2/"
cp upstream/third_party/ffmpeg/COPYING.LGPLv2.1 "$dest/FFmpeg/"
