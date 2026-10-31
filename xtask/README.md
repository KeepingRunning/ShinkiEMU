# ShinkiEMU build tools

Run from the repository root. Cargo subprocesses use `--locked`; update the lockfile deliberately when changing dependencies.

```text
cargo xtask --help
cargo xtask matrix
cargo xtask doctor --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc --all-cores
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --all-cores --release
cargo xtask check --target x86_64-pc-windows-msvc --core rfvp
cargo xtask test --core rfvp
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --out .work/rfvp-package
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc
cargo xtask release --component rfvp --dry-run
cargo xtask metadata validate
cargo xtask metadata build --out .work/metadata-snapshot
cargo xtask ci --target aarch64-apple-ios --component manager --report .work/ios-report.json
```

`build/cores.toml` owns core identity, pinned external source repository/branch/revision, manifest, features, native prerequisites, license inputs and supported/blocked targets. `build/targets.toml` owns target identity, runner and Manager support. `matrix` emits every combination, including blocked rows. Runner labels are from [GitHub's hosted runner reference](https://docs.github.com/actions/reference/runners/github-hosted-runners): macOS Intel uses `macos-15-intel`, arm64 uses `macos-15`.

`doctor` inspects installed tools, source revisions and the selected component's dependency trees; missing prerequisites fail without installation. `deps` is the explicit network/preparation command. No selection means Manager prerequisites; `--core` prepares that core, and `--all-cores` prepares supported cores individually. External core repositories are ignored working checkouts rather than product submodules. `deps` clones/fetches the configured fork and checks out its exact pinned commit, initializes that fork's own submodules, and refuses local modifications instead of resetting them. CPAL remains a small product submodule; mlua uses the registry release.

Native requirements are scoped per component. Only consumers prepare the pinned vcpkg subtitle/FFmpeg trees or DXC runtime. Subtitle lookup runs locked Cargo metadata for the selected core's own manifest and enabled build features, then uses triplets from that exact RoukakuEngine Git checkout with the workspace-owned, baseline-pinned libass manifest; it does not infer optional dependencies from the desktop Manager graph or a neighboring checkout. Android uses JDK 27, SDK 37.2, NDK 30.0.16248370 and private cargo-ndk 4.1.2. No command installs system packages.

Android FFmpeg bindings use the target's versioned API 28 triple and NDK sysroot in both `BINDGEN_EXTRA_CLANG_ARGS_<target>` forms (hyphens and underscores). Bindgen prefers the hyphenated form, preserving the API level when cargo-ndk 4.1.2 replaces the underscored form with sysroot-only flags.
Android Manager builds import `cargo ndk-env --json` and run Cargo directly. This preserves Cargo's artifact and build-script JSON, including every embedded core's resource/license manifest; cargo-ndk's build wrapper consumes those messages.
Portable resource filenames preserve printable ASCII punctuation and internal spaces, except Windows-reserved characters. Leading/trailing spaces, control characters, traversal and Windows device aliases remain rejected; package copy/archive/extraction preserves resource bytes.
Embedded core license inputs use the same checked asset traversal as native resources, preserving inventoried notice directories as well as individual license files.

Apple RFVP and the iOS Manager prepare static libass 0.17.5 on a native macOS/Xcode host with the same vcpkg baseline and pinned RoukakuEngine triplet overlay. The built-in Apple triplets map both Darwin architectures and all three iOS triples to target-isolated installs. No Apple GStreamer tree is needed. Static Manager dependencies and exact Git pins must match `static_manager_targets` in the inventory; dynamic export features stay disabled. iOS packages include all five Rust cores' licenses and an `embedded-cores.json` source inventory.

Impacto's pinned `libatrac9` and `magic_enum` sources live under `.work/native-downloads/`. RenPy preparation downloads its official SDK, CPython 3.12.7, Cython 3.2.4, Cython source-license files and SDL 2.0.20 headers. Windows uses the pinned NuGet development tree and cp312 wheel. Linux builds the pinned CPython source with the host C compiler and loads Cython's pinned Python sources from its standard library. Both interpreters are task-owned under `.work/native-downloads/python`; no system interpreter or developer cache is used. Linux preserves archive executable modes and verifies the SDK's `librenpython.so`, which contains CPython as well as Ren'Py and SDL. Build environments receive `RENPY_SDK_ROOT`, `RENPY_PYTHON_DEV`, `RENPY_SDL_INCLUDE`, `Python3_EXECUTABLE` and `RENPY_CYTHON_LICENSE` automatically on either host.

Linux SDL headers come from the pinned source archive and run its configure
step to generate the native `SDL_config.h`; the Visual C++ archive contains
Windows-only configuration headers and is used only on Windows.

Every native download is SHA256-pinned in `build/dependencies.toml`. Extraction is staged, rejects unsafe paths/links/collisions, and publishes only a complete tree. A local verification record binds every file to the archive recipe. A verified existing tree is reused without downloading; modified trees fail without overwriting their files. Bytecode writes are disabled for the task-owned build interpreter so builds do not mutate the verified installation.

Native subprocesses use `.work/vcpkg-binary-cache`, `.work/vcpkg-downloads` and `.work/tmp` rather than profile caches, with vcpkg concurrency capped at 4 and install operations waiting for the shared lock. The vcpkg registry checkout has complete history: historical port trees must be available locally, so registry acquisition never uses a partial or shallow clone. CMake respects an explicit `CMAKE_GENERATOR`; otherwise it prefers Ninja when available inside an MSVC developer environment.

Each core is built through its own manifest and lockfile into `.work/target/<target>/<plugin_id>`. On Windows, long prefixes use `.work/t/<repository-target-component-hash>` inside the checkout and are checked against an owner record. Set `SHINKI_EMU_TARGET_ROOT` to choose another writable short root. CMake receives ordinary Windows paths after removing Rust's verbatim-path prefix. The build receipt in `.work/build` records the actual target directory, Cargo JSON artifact, build-script `OUT_DIR/core-build.json`, revision, toolchain and features. Native adapters must use `shinki-emu-build` and record the actual CMake DLL, resources and licenses. An absent manifest, incorrect ABI/architecture, missing query export or failed supported build is an error.

Build receipts record whether source worktrees changed or were dirty during compilation. Local development packages retain that provenance; Release preparation requires a clean receipt at the configured source revision. Corresponding-source archives include the selected fork's pinned repository/revision, not whichever branch happens to be checked out.

Packaging consumes release build receipts. Output must be new; staging is in the same parent and publication uses a no-replace rename. Paths, links/reparse points, collisions, hashes, binary architecture and runtime dependency closure are checked without loading binaries. Dependency search is restricted to explicit build/redistributable trees, never the developer's system DLL directory. Cores are independent packages; Manager packages have an initially empty `cores` directory and use the same validated Metadata snapshot as online clients. Android packaging stages JNI/AAR/assets/licenses in a private directory and invokes the pinned Gradle wrapper. Without signing material it produces a clearly named debug-signed CI APK, excluded from Releases. Release signing reuses `SHINKI_EMU_ANDROID_KEYSTORE` (path) or `SHINKI_EMU_ANDROID_KEYSTORE_BASE64`, plus `SHINKI_EMU_ANDROID_KEYSTORE_PASSWORD`, `SHINKI_EMU_ANDROID_KEY_ALIAS` and `SHINKI_EMU_ANDROID_KEY_PASSWORD`. Partial configuration fails; decoded keys remain temporary and are never packaged.

Release archives are deterministic, include build provenance and have corresponding-source ZIPs containing downloaded source archives and their vcpkg ports/patches. Source downloads must match the installed package's SPDX SHA512 values; port recipes and patches must match its recorded SHA256 values. Release sources must be committed, and aggregated platforms must share the same product revision. License or source prerequisites stop publication. A core marked `publish_blocked` may compile but its binary is not uploaded. Resolve its concrete distribution review before changing that field.

`release` writes GitHub only with `--publish` and `GH_TOKEN` or `GITHUB_TOKEN`. Existing published tags/assets stay immutable. Manager/self-owned cores use their own SemVer; external core versions are verified upstream tag/branch plus a digest of full upstream/adapter provenance and their increasing publication sequence. Omit `--version` for external cores; a supplied incompatible version is rejected. `release --dry-run` verifies the real refs without building/publishing. Fan-out resolves each component independently. Indexed `cores-v2.json` adds typed identities while `cores.json` preserves the legacy v1 install catalog. See the [identity encoding and ordering contract](../docs/en/build-and-release.md#package-and-release). CI records declared unsupported rows as `blocked`, with `artifact: null`; supported build/package failures remain failures.

Manual `ci.yml` dispatch accepts `since`, using the existing
`cargo xtask matrix --json --ci --since <ref>` input classifier. The matrix
checkout fetches full history so the comparison ref is available. An optional
`components` comma-separated list narrows that affected matrix. For example:
`gh workflow run ci.yml --ref feat/unblock-renpy-linux -f since=ee20b99 -f components=renpy`.
Empty inputs, pushes and pull requests retain the full matrix.

Tool behavior tests: `cargo test --locked -p xtask -p shinki-emu-build`. Tests compile their own public synthetic shared libraries; no game installation or private data is used.
