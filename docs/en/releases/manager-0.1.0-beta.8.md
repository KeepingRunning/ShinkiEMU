# ShinkiEMU Manager 0.1.0-beta.8

New Musica patches require Musica core beta.5 or later. Manager checks the actual loaded core's native capabilities and prompts for a core update when requirements are missing. Yorino requires both capability bits 256 and 512.

Online catalog and local Luau/ZIP patches share execution, release-fingerprint binding and transactions. Output defaults to a separate new directory; in-place replacement supports backup and restoration. Patches convert resources and cannot modify or create EXE files. After patches-v2 opens, older Managers continue reading the retained v1 catalog and legacy scripts.

This release adds update checks and uses the compile-time product version for the CLI, About view, update requests and package identity. Known beta.7 issue: its official Windows binary self-reports beta.6. Published beta.7 tags and assets remain unchanged; identify that older release by its release page.

Core versions advance independently of Manager's beta sequence. Update the required cores before applying new patches. Every platform package includes corresponding source and original dependency notices. Android uses the official release certificate, macOS/iOS use ad-hoc signatures, and the OHOS HAP uses the OpenHarmony SDK test PKI. Device launch checks are waived; OHOS retail-device installability is unverified.
