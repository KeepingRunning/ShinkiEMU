# ShinkiEMU Metadata

This directory is the sole source for the public metadata snapshot. Data is CC0-1.0; Luau recipes are MIT OR Apache-2.0 (see LICENSE).

- Compatibility: `compatibility/entries/<vid>/<rid>/<core>/<platform>-<architecture>-<variant>.json`.
- Core releases: `engines/cores/<core>/<platform>/<version>.json`.
- Presets: `engines/presets/<rid>/<core>.json`.
- Patches: release-bound `patches/entries/<id>.json` using `shinki.emu.patches.v2`; reviewed Luau scripts remain in `patches/`.

`cargo xtask metadata schema` exports schemas from the Rust DTOs. `cargo xtask metadata validate` checks schemas, paths, references and limits. `cargo xtask metadata build --out <new-dir>` generates a deterministic snapshot with `patches-v2.json`, release-binding sidecars (`patches/<script-stem>.patch.json`) and a SHA256 index tied to the source commit. Generated snapshots are not source data.

Every catalog patch is an offline embedding recipe with nonempty concrete `releases` and a `downloads` declaration array. Each release uses the existing VNDB `release_id`/`vn_id`, localized title, actual edition label, optional actual game version and a nonempty `{path,size,sha256}` source fingerprint group. Manager identifies exactly one matching release before creating writable transaction state in either output mode. Local `.luau` scripts or ZIP packages use the same executor, are labelled unreviewed, and may run without a release manifest; those scripts validate their own inputs. Optional local release bindings use `<script-stem>.patch.json`.

Every recipe supports a new independent directory (default) and optional in-place overwrite through the existing `.shinki/backup` transaction. Manager and its CLI (`patch apply --in-place`, `patch restore`) retain backup/restore support; failure restores changed game files. New-directory output does not copy saves. ZIP packages require root `patch.luau` and safe relative modules/resources; `.luau` modules use `ctx.load`, and ZIP resources are read-only through `ctx.open("package", path)`. `ctx.musica_embed(profile, translations, resource_paths, destination)` materializes native SC with Musica's existing native readers; no runtime translation table is installed. Core semantic requirements use the existing output-profile `required_capabilities` gate.

Legacy `patches.json` (`patches.v1`) and `patches/catalog.json` are frozen from `legacy/`, with their scripts under generated `patches/legacy/`. They remain usable by old Managers. New Managers consume only `patches-v2.json`; they do not fall back to work-only or multi-category v1 data. Existing core v1 and compatibility v4 catalogs keep their established rules.

`ctx.download(url, sha256, size)` accepts HTTPS without credentials, a mandatory SHA256 and size 1..1073741824. It writes only verified hash-named files under game `.shinki/patch`, returns a read-only Source handle, checks cached files again and removes failed partial downloads. Catalog scripts may download only exact URL/SHA256/size triples in their entry's `downloads`; missing/empty declarations allow none. Local scripts have the same protocol/hash/size restrictions without a catalog allowlist. `ctx.check(label, passed, detail)` reports a check and aborts on failure. `ctx.fail(code, message)` and native Luau `error(message)` abort the transaction; Manager retains the error text and reports rollback. No process execution, ambient filesystem access or other networking is exposed.

Use the compatibility or Luau patch issue form. Intake validates fields and compiles Luau syntax without running the script, then opens a review PR containing the entry and `Metadata/patches/<id>.luau`; maintainers approve before merging. No trial mode, synthetic-download fallback or second executor is present.
