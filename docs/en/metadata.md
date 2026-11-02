---
title: Metadata contributions
description: Compatibility v4, reviewed patch entries and generated snapshots.
---

# Metadata contributions

## Submit a report

Use the [compatibility or patch issue form](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new/choose). Compatibility reports need VNDB vID/rID, exact core ID and version, Manager version, platform, architecture, variant, status and a short reproduction. Report only observed behavior and omit game text, keys, commercial assets and saves. A build report cannot create a playability conclusion.

`shinki.emu.compatibility.v4` uses the key:

`(release_id, emulator, platform, architecture, variant)`

Platforms are `windows`, `linux`, `macos`, `android`, `ios`, `ohos`; architectures are `x86_64`/`aarch64`. Desktop platforms use `desktop`; mobile platforms use `device`/`simulator`. Core IDs are case-sensitive. Status is `perfect`, `completable`, `flawed`, `boot_only` or `unplayable`. Records for different targets coexist; missing target records are unreported.

Source entries live at `Metadata/compatibility/entries/<vid>/<rid>/<plugin_id>/<platform>-<architecture>-<variant>.json`. Old records without an explicit architecture require evidence before publication; do not guess.

## Luau patches and permission

Patch entries live at `Metadata/patches/entries/<id>.json`. `patches-v2.json` uses `shinki.emu.patches.v2`; every entry references a reviewed relative `.luau` script under `Metadata/patches/`. There are no recipe/external/hosted categories in v2. Provide an ID, semver version, localized `title`/`summary` maps (`zh-CN` and `en` required, `ja` optional), exact core IDs, source, license, authorization and issue references.

Each `releases` record binds a concrete VNDB `release_id` and `vn_id` to its title, edition, known game version and file fingerprints (`path`, `size`, `sha256`). Do not invent missing edition/version facts. Manager verifies the selected release before creating a writable transaction.

Scripts may call `ctx.download(url, sha256, size)`. Catalog entries must declare each allowed HTTPS URL, SHA256 and byte size in `downloads`; an absent or empty list permits no downloads. Sizes must be between 1 byte and 1 GiB. Verified hash-named files are cached under `<game>/.shinki/patch/`; redirects remain HTTPS and cached files are rechecked. A license label alone is not redistribution permission.

Online and local Luau/ZIP patches use the same VM and transactions. Local scripts without catalog binding are marked unreviewed. Output defaults to a separate directory; in-place mode supports backup and hash-checked restoration. Resource patches cannot create or modify EXE files. Musica's new native patches require core beta.5 or later; Manager checks the actual descriptor's required capability bits, including 256 and 512 for Yorino.

Older Managers retain `patches.json`, `patches/catalog.json` and scripts under `patches/legacy/`, generated from `Metadata/legacy`. They do not consume v2 entries.

## Maintainer review

A maintainer with write/maintain/admin permission reviews the issue and applies `compat:accepted` or `patch:accepted`. Intake treats form text as data, records the raw body SHA256 and `updated_at`, and proposes the keyed entry through a normal PR. It never executes issue scripts. Editing the approved body requires removing approval and reviewing again; recheck the binding before merge. Review schema/path consistency, duplicate keys, cross-references and permission evidence. The local intake command generates candidate files and `approval.json`; it does not itself grant approval or publish an attachment.

## Core release catalog

Publishing a core release opens a catalog PR with one entry for each released platform. The trusted metadata workflow validates those entries against the repository schema and automatically squash-merges the PR when validation passes; the portal's scheduled build then publishes the updated snapshot consumed by Manager. If the PR remains open, check the metadata validation workflow and its permissions instead of editing a second catalog.

## Commands and generated output

```sh
cargo xtask metadata schema
cargo xtask metadata validate
cargo xtask metadata build --root Metadata --out .work/metadata-snapshot
cargo xtask metadata resolve-core --plugin-id rfvp --platform windows-x86_64 --abi shinki.emu.native.c.v3
cargo xtask metadata intake --kind compatibility --event event.json --out .work/compat-candidate
cargo xtask metadata intake --kind patch --event event.json --out .work/patch-candidate
```

Schema exports default to `Metadata/schema` and come from Rust DTOs. Edit source entries and scripts, not generated parallel schema rules. Validate/build accept `--root`; build/intake require a new output directory.

Snapshot output contains `compatibility.json`, `cores.json`, `presets.json`, `patches-v2.json`, legacy `patches.json` and `patches/catalog.json`, reviewed current/legacy scripts and `index.json`. Core release entries come from `Metadata/engines/cores/<plugin_id>/<platform>/<version>.json` and only reference this repository's release downloads. Index SHA256 hashes/sizes and source commit cover generated files. Commit time supplies the timestamp, making the same committed input deterministic. Limits are 8 MiB per file, 64 MiB total and 4096 files; large downloads remain outside this budget.

The index also covers `cores-v2.json` (`shinki.emu.cores.v2`), containing both authentic legacy entries and new external `identity` records. `cores.json` remains legacy v1 and retains published downloads for older Managers. New consumers prefer indexed v2 and fall back to v1 only when v2 is absent. Upstream/adapter identities, safe filenames and sequence-based latest selection are defined in [Build and release](build-and-release.md#package-and-release).

Manager and the portal consume the same snapshot at `https://project-shinkirou.github.io/data/shinkiemu/v1/`. Keep `Metadata/` as the single source; do not hand-edit a second compatibility/core status table. See [User guide](user-guide.md) and [Build and release](build-and-release.md).

Complete external core identifiers can reach 88 bytes. The index therefore adds `compatibility-v5.json` (`shinki.emu.compatibility.v5`, core version limit 88; Manager version limit remains 64). New consumers prefer that indexed file. `compatibility.json` remains v4 with only reports whose core version fits its original 64-character limit, preserving a valid entire snapshot for old Managers; source reports are retained unchanged.
