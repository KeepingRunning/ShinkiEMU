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

## Patch kinds and permission

Patch entries live at `Metadata/patches/entries/<id>.json`. The generated catalog uses `shinki.emu.patches.v1`. Provide an ID, semver version, localized `title`/`summary` maps (`zh-CN` and `en` required, `ja` optional; the portal falls back to `en`), work/release IDs, exact cores, kind and license. Supply HTTPS source/authorization links and an issue reference where applicable.

- **recipe:** references a safe relative `.luau` script under `Metadata/patches/`. The script must be reviewed repository content. Only recipes enter the executable recipe menu; users explicitly select execution.
- **external:** supplies an artifact or source URL. It is displayed as a link/download, not run by the product.
- **hosted:** requires a public HTTPS authorization record and an artifact with URL, SHA256, byte size and safe filename. The URL must be under `https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/`. The default maximum is 1 GiB per artifact. A license label alone is not evidence that someone else's patch can be redistributed.

Hosted release naming follows `patch-<id>-v<semver>`. Attachments are separately reviewed distribution artifacts, not files embedded in the metadata snapshot. Updating a version requires binding its exact bytes and authorization again. An empty `release_ids` list means the submitter declared work-level applicability; display “release not restricted,” not “all releases verified.”

Manager offers a download action for applicable hosted artifacts and external entries that include an artifact. It follows HTTPS redirects only (hosted release assets retain the GitHub release CDN allowlist), writes a temporary `.part` file, verifies byte count and SHA256, then publishes `<game>/.shinki/patch/<file_name>` atomically. A previously verified artifact is reused. The download directory becomes patch input; the artifact never runs as a script. In-place recipes back up changed originals under `<game>/.shinki/backup/<id>/` with a manifest and support hash-checked restore.

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

Snapshot output contains `compatibility.json`, `cores.json`, `presets.json`, `patches.json`, recipe-only `patches/catalog.json`, reviewed scripts and `index.json`. Recipe titles in the executable catalog use `zh-CN`. Core release entries come from `Metadata/engines/cores/<plugin_id>/<platform>/<version>.json` and only reference this repository's release downloads. Index SHA256 hashes/sizes and source commit cover generated files. Commit time supplies the timestamp, making the same committed input deterministic. Limits are 8 MiB per file, 64 MiB total and 4096 files; large attachments remain outside this budget.

The index also covers `cores-v2.json` (`shinki.emu.cores.v2`), containing both authentic legacy entries and new external `identity` records. `cores.json` remains legacy v1 and retains published downloads for older Managers. New consumers prefer indexed v2 and fall back to v1 only when v2 is absent. Upstream/adapter identities, safe filenames and sequence-based latest selection are defined in [Build and release](build-and-release.md#package-and-release).

Manager and the portal consume the same snapshot at `https://project-shinkirou.github.io/data/shinkiemu/v1/`. Keep `Metadata/` as the single source; do not hand-edit a second compatibility/core status table. See [User guide](user-guide.md) and [Build and release](build-and-release.md).

Complete external core identifiers can reach 88 bytes. The index therefore adds `compatibility-v5.json` (`shinki.emu.compatibility.v5`, core version limit 88; Manager version limit remains 64). New consumers prefer that indexed file. `compatibility.json` remains v4 with only reports whose core version fits its original 64-character limit, preserving a valid entire snapshot for old Managers; source reports are retained unchanged.
