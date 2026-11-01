---
title: Core ABI and SDK
description: C ABI v3 lifecycle, owner threads, CPU leases and core packages.
---

# Core ABI and SDK

## Protocol and entry point

The native protocol is `shinki.emu.native.c.v3`, with numeric ABI version **3**. Dynamic cores export `shinki_emu_query`; static integration registers the same Query function shape. The Rust contract is `Source/FamilyApi/shinki-emu-abi`, and its generated C header is `include/shinki_emu.h`. C++ consumers use this header rather than maintaining another ABI definition.

Only C-layout values cross the boundary: versioned headers, bounded borrowed spans, opaque handles, status codes and function pointers. No Rust-owned containers or unwinding cross the ABI. Libraries and immutable descriptor storage remain resident. Query accepts a version and an output table pointer; unsupported versions leave the output null. Query does no I/O, starts no workers and installs no logger.

## Lifecycle

1. **query** obtains the immutable `CoreApi` table; validate the version, size and required function pointers.
2. **initialize** attaches diagnostics before ordinary core activity.
3. **describe** returns family/plugin identity, version, formats, typed configuration and capabilities. CPU surfaces are required. A nonzero render extent is legal only when the descriptor declares and bounds it.
4. **probe** examines a game directory. `NO_MATCH` is normal; probe strings borrow module storage until the next ordinary call, so the host copies needed metadata first.
5. **open** validates configuration and creates a native session. A failed open cleans up and returns a null session. Successful output includes session/context/output identities and surface metadata.
6. **input** enqueues owned, ordered input; **control** requests pause, resume or a retained retry-safe operation. Pointer coordinates refer to the displayed frame's raster pixels and identity.
7. **state** and **frame** read published state and surfaces. Polling never advances the VM. Failed sessions expose typed errors; retry is offered only when the core declares it safe.
8. **cancel** may run concurrently and must wake blocked native services. It must not wait behind the operation it is cancelling.
9. **close** consumes the handle on every outcome, joins workers and ends callbacks before returning. The host blocks new cancellation calls and waits for in-flight cancellation before closing.
10. **extension** negotiates a named, versioned C table. Unknown extensions return `UNSUPPORTED` with null output. `shinki.module_owner` provides an optional owner for real cross-session resources, created/destroyed on the module owner thread after sessions end.

Ordinary ABI calls are serialized on the host's owner thread. The core's native owner drives its own VM, render/audio scheduling and resource lifetimes. Manager supplies input, control and host services; there is no host advance callback. Cancellation and epoch checks reject late results from obsolete sessions or output generations.

The shared Rust diagnostic bridge forwards each original event once. If a host callback emits tracing events into the same statically linked subscriber, those nested events are not forwarded back through the ABI. A thread-local guard covers formatting and the callback, resets on return or unwind, and leaves other threads and subsequent events independent. This applies to every core using `shinki-emu-abi/diagnostic-bridge`, without changing log levels or worker stack budgets.

## Converted-game compatibility

The existing `Descriptor.capabilities` mask has two optional semantic bits; ABI
version 3, its C layout and fingerprint are unchanged:

- `CAP_NATIVE_SCRIPT_IDENTITY` (64): validate an explicit converted-script hash,
  isolate that script's saves/read state, and retain the existing namespace for
  markerless games.
- `CAP_CONFIGURED_FONT_PRIORITY` (128): a configured font takes priority over
  authored/restored primary face choices, including ruby and UI text.

A Luau recipe can return `required_capabilities = 192` beside `plugin_id` and
`configuration` in its output profile. The receipt retains this requirement.
Manager checks the selected, actually loaded core's `describe` result before
adding an imported game or binding its profile, and again before opening a session.
Automatic import also rejects a receipt whose requirements changed after conversion.
Missing capabilities fail closed with a core-update diagnostic; unknown requirements
reject the profile rather than being ignored. Version strings and filenames
are not feature evidence. Legacy profiles omit the
field and require zero bits; zero is also omitted from new serialized receipts.

## Text replacement levels

The descriptor capability bits are the source of truth: no `CAP_TEXT_HOOK` means **none**; `CAP_TEXT_HOOK` alone means **synchronous**; `CAP_TEXT_HOOK | CAP_ASYNC_TEXT` means **async-v1**. `CAP_ASYNC_TEXT` without the hook bit is invalid. The nine pinned external cores currently declare none; external cores may adopt synchronous replacement without changing their scheduling model. Ownership defines SDK cores: Musica is SDK-owned; Artemis remains external despite using SDK audio. CMVS is maintained separately.

ABI version remains **3**. `OpenRequestTextV1` embeds the unchanged base `OpenRequest` and appends a pointer to `AsyncTextServiceV1` (version 1 and explicit size). The host sets the base header size to cover the wrapper. Cores read the tail only when that size covers the wrapper, then validate the service version, size and callbacks. A null pointer disables replacement. Base-only external binaries keep using the unchanged `HostServices.text` synchronous callbacks. An async-v1 core never falls back to the synchronous path.

`submit` copies bounded text items before returning; `poll` returns `PENDING` immediately until completion. Only text advance waits: rendering, animations, input and audio keep running. Results retain the replacement semantics, item IDs and `(session_epoch, context_epoch, request_id)` identity. Request IDs increase within a context. Repeating an unacknowledged submit is idempotent. `CAPACITY` reports the required caller-owned buffers without executing the provider or extensions twice; completed output remains available until `acknowledge`. `cancel` retires only the matching request, while `invalidate` advances the context and discards old requests, source context and provider cache. Callbacks may not wait for network or script completion.

Manager uses the provider's `AsyncTranslationService` behind both paths; the synchronous compatibility callback waits for the same result. Musica submits once and polls with retained buffers from its native pump, without suspending audio. Enter, Space, primary click or Ctrl skips pending translation and displays the original replacement input; the input is consumed so it does not also advance that line. Skip mode also skips pending translation. Skip, load, rollback and close discard late completions; retries retain the failed text event and identity without rerunning the VM.


## CPU surface leases

A Ready `FrameSurface` carries actual dimensions, stride, pixel/color/row metadata, identity and one immutable CPU lease. Validate the metadata and epoch before adopting it. Pending and Unchanged frames do not carry pixel ownership. Preserve the declared raster/logical geometry and color interpretation.

Return each lease exactly once, only after the real upload API no longer reads its pixels. The lease's owner and release callback remain valid after session close and on another thread; release must not touch the VM, device, window, session or host services. Retaining a frame does not keep gameplay alive, nor permit overwriting leased pixels. The ABI's `CpuFrame` wrapper releases an adopted lease even when validation fails.

## SDK ownership

`Source/SDK/shinki-emu-sdk` contains shared mechanisms, not a second game runtime. Its `runtime` values stay inside the process. `native` shares configuration validation, epochs, cancellation, window-result rejection and independent CPU leases; `native_input` copies bounded input without running the VM. `NativeAudioOutput`/`NativeAudioConverter` serve core-owned device threads; `NativeAudioOutput::progress()` reports accepted and audible frames on the 48 kHz source timeline, net of device and resampler latency, so cores can drive their media clocks. `progress_reader()` returns a cloneable `NativeAudioProgressReader` that reads the same timeline from other threads while the device thread is blocked or idle. `write` accepts any length and carries a partial device block; call `flush()` before the device thread idles so every accepted frame becomes audible (the silence padding counts as accepted). Mixing, media clocks, native save formats and game rules remain with each core. Use the [SDK reference](https://github.com/Project-Shinkirou/ShinkiEMU/tree/main/Source/SDK/shinki-emu-sdk) for concrete APIs.

## Core package

A package contains a directory named after the library stem:

```text
<library-stem>/
  core.json
  shinki_emu_<core>.dll       # or libshinki_emu_<core>.so/.dylib
  build.json
  Resources/                 # when the native core requires it
  licenses/
  <native dependency files>
```

`core.json` uses `shinki.emu.core_package.v1` and records `plugin_id`, `library`, `version`, `abi` and file paths/SHA256 hashes. The file inventory includes every payload file except the manifest itself. Libraries require the new filename prefix and query symbol. Packaging validates architecture, dependency closure and licenses, rejects path traversal/links/case collisions, and publishes only to a new output directory. Manager verifies the package before installation; older product protocols and loader aliases are not accepted. See [Build and release](build-and-release.md).

New external packages add `identity` with original upstream and organization adapter provenance; `version` must equal its computed safe identifier. Legacy packages omit it and retain their actual SemVer. New Manager checks catalog/package plugin, version and identity together before staging, and displays the complete upstream/adapter label. The C ABI and descriptor version remain independent. See [version identity rules](build-and-release.md#package-and-release).
