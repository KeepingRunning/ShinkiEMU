# ShinkiEMU Manager 0.1.0-beta.6

Musica uses the original eden/Perseus `.sav` slots and matching `.png` thumbnails for manual saves, quick saves and autosaves. Old beta `.asav` files are incompatible and remain untouched in their old directories. There is no conversion or fallback reader.

Musica is also statically embedded in the iOS Manager for the device and both simulator architectures, alongside its Android and experimental OHOS integrations. iOS does not support installing a separate dynamic core. The integrated release requires its own exact-head build, package, signature, native-dependency and corresponding-source gates; isolated-port results do not replace those gates.

The Android Manager embeds six cores: RFVP, Siglus, Musica, Artemis, Impacto and Ren’Py, for both ARM64 and x86_64. KiriKiri is not included; its Android static embedding remains in development for a later Manager release. A standalone Android core ZIP is not evidence of Manager support. The existing iOS core set is retained; the newer KiriKiri/Ren’Py iOS ports are not included.

External core packages now retain the original upstream ref/commit and the adapter identity. This Manager consumes the typed package/catalog format; beta.5 and earlier continue using the unchanged legacy packages and catalogs.

On desktop, **Choose output** opens the native folder picker. Select the parent location and review the proposed new `localized-output[-N]` directory. Selection does not create that directory; a successful conversion publishes it without overwriting an occupied path.

Converted-game profiles can require native core capabilities. Manager checks the actual loaded descriptor before adding or binding the converted game, then checks the retained receipt again before launch. Missing requirements request a core update; malformed or unknown masks are rejected. Legacy profiles require no additional bits.

Sakura recipe 0.2 uses native GBK script text, archive filenames and an explicit HanaMinA primary font, without the old runtime translation table. It requires the updated RFVP core with isolated script saves and configured-font priority. Update the core and create a fresh, separate conversion; existing converted directories are not rewritten. The new script identity does not reuse old Japanese/external-table saves or read history. Original subtitle styles and local font inputs remain intact.

The OHOS HAP remains experimental and signed with the OpenHarmony SDK test PKI. Device/emulator/game/account launch checks are waived; retail-device installability is unverified. Corresponding source and dependency notices accompany every published platform package. Published Manager beta4 and beta5 assets remain unchanged.
