---
title: impacto
description: Formats, native behavior and limits of impacto.
---

# impacto

`plugin_id: impacto` · Engine family: MAGES.

Upstream: [CommitteeOfZero/impacto](https://github.com/CommitteeOfZero/impacto). Adaptation fork: [Project-Shinkirou/impacto, shinki branch](https://github.com/Project-Shinkirou/impacto/tree/shinki).

## Formats and execution

impacto retains its MAGES SC3 VM, native resource readers and game-specific configuration. The C++ integration remains under `src/shinki`, with Cargo/CMake build entry under `shinki`. Original OpenGL composition, OpenAL audio, FFmpeg movies, game UI and save behavior remain in the native engine.

Bundled assets stream to app-private storage with a 32 MiB per-file limit, accommodating the upstream Noto CJK bold font (17,032,620 bytes) without allocating a full font-sized buffer.

## Storage

`userconfig.toml` stays in Impacto's platform preference directory, which is
`%APPDATA%/Committee of Zero/Impacto` on Windows. The selected CC profile sets
`SaveDataType.None`, so it has no profile save format or save files. Any future
profile with native save support keeps the upstream profile-defined location.

The retired `save_root` was user-selected. To retain a beta.1/beta.2 configuration,
close the game and copy `<old-save-root>/userconfig.toml` to the platform preference
directory only if the destination file does not already exist. The Manager shows
the retired path once. There is no automatic guessed-path migration or fallback
when the upstream directory is unwritable. The adapter's packaged base paths use
`<resources>/saves`, matching the upstream sample's `RootSavesDir = "./saves"`;
this has no save-file effect for the supported CC profile.

Storage comparison base: upstream `3706ddd89d4ad4aed0e32e127355c4251f22e574`.
The native `userconfig.cpp`, `io/filemeta.cpp`, `profile/basepaths.cpp` and CC
save profile are unchanged. The adapter clears its previous session's
`UserConfigPath` and calls native `UserConfig::Configure()` after installing its
packaged CC definition. Only its CPU-frame backing size/window mode and
unsupported patch selection are overridden in memory; save/config paths and
native error handling are not redirected.

## Native behavior and limits

The adapter exposes C ABI v3 and CPU frame ownership rather than reimplementing MAGES scripts in Manager. Native DDS parallel-read state and movie-audio release are ownership boundaries preserved by the fork. Steam font resources are read from the user's actual game DDS files, not distributed as replacement assets.

The open-source dependency path does not bundle or load Bink DLLs. Game-specific system-data failures and unexercised routes remain separate compatibility concerns; a successful query does not establish that a title's system page, movies or saves work. ISC and component-specific licenses both apply; binary distribution requires a complete native dependency and corresponding-source review.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
