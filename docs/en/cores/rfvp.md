---
title: RFVP
description: Formats, native behavior and limits of RFVP.
---

# RFVP

`plugin_id: rfvp` · Engine family: FVP

Upstream: [xmoezzz/rfvp](https://github.com/xmoezzz/rfvp). Adaptation fork: [Project-Shinkirou/rfvp, shinki-sakura-native branch](https://github.com/Project-Shinkirou/rfvp/tree/shinki-sakura-native).

## Formats

FVP uses HCB bytecode. The first little-endian u32 is the system-descriptor offset; code begins at byte 4 rather than after a magic string. The descriptor supplies the entry point, globals and syscall table. Length-prefixed C strings include their terminating NUL; encoding is an explicit core setting.

## Native behavior and limits

The fork retains its App, VM, GPU renderer, device audio and saves. The adapter delivers the final scene through immutable CPU leases; Manager does not reproduce syscalls or drive a host tick. Translated HCB literals use the ordinary script reader and explicit encoding; external translation tables and runtime replacement are removed. `font_path` selects the native primary face consistently for names, dialogue, ruby and native UI. Movie/BGM ASS mappings retain explicit fonts and original media clocks.

The Android static Manager graph shares wgpu 30 with Siglus and Manager. Native WMV decoding uses the Rust API; its optional `c-abi` exports stay disabled in Manager.

WMV/ASF/WMA playback uses the fork's native movie/decoder path. Requested native fonts and game-specific resources remain real inputs. Recognizing an HCB file does not establish syscall coverage or edition compatibility.

## Save paths and format

The original RFVP engine stores numbered saves in `<game>/save/sNNN.bin` and global HCB data in `<game>/save/save.bin` (`crates/rfvp/src/script/global.rs:134-136` and the `setsumei/` reverse-engineering notes). The fork's `SaveItem` appends its own runtime-state block after the original header, and its global file wraps `GlobalSaveDataV1` in an `RFVG` envelope, so neither file is a byte-level implementation of the original format. The fork keeps its own names, `<game>/save/rfvp_sNNN.bin` and `<game>/save/rfvp_global.bin`, and still reads older `<game>/save/sNNN.bin` files. These saves belong to the game directory and are not moved into `.shinki`.

Sakura conversion recipe 2 writes native GBK literals, including names/ruby/long lines, and uses the locally supplied HanaMinA face. Growing literals use standard `Jmp`/`PushString` islands; original instruction/call/thread-entry PCs stay fixed. New literal addresses create a new read identity. The output's `shinki-native-script.json` contains only version 1 and the full HCB SHA-256, never text or PC mappings. After validating it against the loaded HCB, RFVP isolates this output's saves/read bitmap under `<game>/.shinki/rfvp/<hash>/save`; mismatches fail startup. Unmarked games keep the paths above. Do not reuse old Japanese/external-table saves or read history. Convert into a separate output directory and retain original data. Fonts and subtitles remain private local inputs with their existing copyright/license terms.

The same GBK setting also decodes BIN resource names. The recipe transcodes standard archive name tables from CP932 to GBK, requiring unchanged encoded widths, unique native lookup names and valid entry/payload bounds; payload bytes and offsets remain unchanged. Unrepresentable names, width changes and collisions fail conversion. HanaMinA's pinned bytes match the [public Hanazono font](https://github.com/googlefonts/chinese/tree/gh-pages/fonts/HanaMin), dual-licensed under the [Hanazono Font License / SIL OFL](https://github.com/googlefonts/chinese/blob/gh-pages/fonts/HanaMin/LICENSE.txt). Other private patch fonts retain their own terms and are not redistributed.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
