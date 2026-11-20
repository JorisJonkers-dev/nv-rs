# Changelog

## [0.12.0](https://github.com/JorisJonkers-dev/nv-rs/compare/v0.11.0...v0.12.0) (2026-11-20)


### Features

* **host:** the built-in menu-file reader turns itself on when the data folder holds the interface extender's files ([#390](https://github.com/JorisJonkers-dev/nv-rs/issues/390)) ([bc3c852](https://github.com/JorisJonkers-dev/nv-rs/commit/bc3c8524b8043eaa3d63c003a6224c1fb0b50934))

## [0.11.0](https://github.com/JorisJonkers-dev/nv-rs/compare/v0.10.0...v0.11.0) (2026-11-19)


### Features

* the engine on macOS: build, window and a release build ([fc7b393](https://github.com/JorisJonkers-dev/nv-rs/commit/fc7b393105d0d13ad51dbe2d0b9c5c29e0b97fd9))

## [0.10.0](https://github.com/JorisJonkers-dev/nv-rs/compare/v0.9.0...v0.10.0) (2026-11-19)


### Features

* **d3d9-wgpu:** menus follow the mouse with bBackground Mouse=1: the cursor's moves reach the game's window procedure as WM_MOUSEMOVE (slaterain/nv-rs[#766](https://github.com/JorisJonkers-dev/nv-rs/issues/766)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **d3d9-wgpu:** the menus' cursor speed follows Windows' pointer speed, as in the original (slaterain/nv-rs[#767](https://github.com/JorisJonkers-dev/nv-rs/issues/767)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **fork-api:** the installation check as a typed result behind the feature engine ([#373](https://github.com/JorisJonkers-dev/nv-rs/issues/373)) ([6287e65](https://github.com/JorisJonkers-dev/nv-rs/commit/6287e659ad0f0d556ee8bdb31c33df8e485ce27a))
* **mod-chooser:** install mods: a text installer for FOMOD and plain mod archives, the same on Windows, Linux and macOS (slaterain/nv-rs[#750](https://github.com/JorisJonkers-dev/nv-rs/issues/750)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **nvse:** JIP CC&C: CCCOnLoad, CCCSetFloat, CCCSetString, CCCSetTrait, CCCSavedForm, CCCSMS (slaterain/nv-rs[#761](https://github.com/JorisJonkers-dev/nv-rs/issues/761)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **nvse:** JIP LN commands that rewrite the game's code, part 1: ToggleVanityWheel, ToggleCameraCollision, InitRockItLauncher (slaterain/nv-rs[#772](https://github.com/JorisJonkers-dev/nv-rs/issues/772)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **nvse:** JIP LN commands, lane 2 package 4: ShowQuantityMenu, CloseActiveMenu (slaterain/nv-rs[#759](https://github.com/JorisJonkers-dev/nv-rs/issues/759)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **nvse:** JIP's light setters, AttachLight, RemoveLight and SetCursorPos (slaterain/nv-rs[#775](https://github.com/JorisJonkers-dev/nv-rs/issues/775)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **nvse:** with JIP: a sound's playing copies found, stopped and changed (4 JIP commands) (slaterain/nv-rs[#757](https://github.com/JorisJonkers-dev/nv-rs/issues/757)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **nvse:** with JIP: MessageBoxExAlt's boxes up to 1200 wide (JIP game patch G55) (slaterain/nv-rs[#760](https://github.com/JorisJonkers-dev/nv-rs/issues/760)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **nvse:** with JIP: mods' own fonts, 89 fonts instead of 8 (JIP game patch G129, SetFontFile) (slaterain/nv-rs[#771](https://github.com/JorisJonkers-dev/nv-rs/issues/771)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **tweaks:** lStewieAl's Tweaks: mods' settings files in Tweaks\INIs read over the main INI (slaterain/nv-rs[#749](https://github.com/JorisJonkers-dev/nv-rs/issues/749)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))


### Bug Fixes

* **d3d9-wgpu:** AMD and Intel cards on Vulkan: grass drawn as thin blades again (slaterain/nv-rs[#769](https://github.com/JorisJonkers-dev/nv-rs/issues/769)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **d3d9-wgpu:** AMD on Vulkan: a frame with very many passes no longer loses the device (submits of at most 512 passes and commands) (slaterain/nv-rs[#778](https://github.com/JorisJonkers-dev/nv-rs/issues/778)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **engine-platform:** keep the pointer-hold helpers visible to the tests off Windows ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **nvse:** the extenders' call check: unread calls a checked list, ShowMessageBox's closing 0, words counted by type (slaterain/nv-rs[#776](https://github.com/JorisJonkers-dev/nv-rs/issues/776)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **nvse:** xNVSE inline functions no longer stop the game: the lambdas' list copies get the three words they take (slaterain/nv-rs[#765](https://github.com/JorisJonkers-dev/nv-rs/issues/765)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **stewie:** lStewieAl's Tweaks: ten crash fixes (water explosions, multibounds, stealing, regions, limbs, packages, patrols, doors, SetActorRefEssential, saving) (slaterain/nv-rs[#754](https://github.com/JorisJonkers-dev/nv-rs/issues/754)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **stewie:** lStewieAl's Tweaks: ten menu and HUD crash fixes (inventory with nothing selected, container keys, console while menus load, list boxes, level 100, credits, holotapes) (slaterain/nv-rs[#755](https://github.com/JorisJonkers-dev/nv-rs/issues/755)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **stewie:** lStewieAl's Tweaks: twelve crash fixes in animations, AI and equipping (crash group C) (slaterain/nv-rs[#780](https://github.com/JorisJonkers-dev/nv-rs/issues/780)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **viewer:** Linux and macOS: play.sh's profile variable tied to the program's; the play-run test built from play.sh's own exports (slaterain/nv-rs[#777](https://github.com/JorisJonkers-dev/nv-rs/issues/777)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **viewer:** Linux and macOS: the game's window always fits the screen (a framed window too large for it is shown borderless) (slaterain/nv-rs[#774](https://github.com/JorisJonkers-dev/nv-rs/issues/774)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **viewer:** separate Unix save profiles from engine profiling (slaterain/nv-rs[#763](https://github.com/JorisJonkers-dev/nv-rs/issues/763)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))


### Performance Improvements

* **engine-fallout-cell:** shorter saves late in a playthrough: the changed-forms map gets 10,037 buckets (slaterain/nv-rs[#768](https://github.com/JorisJonkers-dev/nv-rs/issues/768)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **engine-fallout-cell:** shorter saves late in a playthrough: the clean-up every save runs first finds expired cells natively (slaterain/nv-rs[#762](https://github.com/JorisJonkers-dev/nv-rs/issues/762)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))
* **engine-fallout-cell:** shorter saves late in a playthrough: the expiry queue's inserts as Rust code (slaterain/nv-rs[#770](https://github.com/JorisJonkers-dev/nv-rs/issues/770)) ([9c0b6e5](https://github.com/JorisJonkers-dev/nv-rs/commit/9c0b6e5f23bfaf863de09239dfe07bca45ae1ebf))

## [0.9.0](https://github.com/JorisJonkers-dev/nv-rs/compare/v0.8.0...v0.9.0) (2026-11-19)


### Features

* **fork-api:** typed in-process facade crate (step N1 of [#353](https://github.com/JorisJonkers-dev/nv-rs/issues/353)) ([#366](https://github.com/JorisJonkers-dev/nv-rs/issues/366)) ([521fd79](https://github.com/JorisJonkers-dev/nv-rs/commit/521fd790bc8f463e38091ce9ac8a300bad895ca4))
* **fork-play:** play-mode entry point of the engine (step N3a of [#353](https://github.com/JorisJonkers-dev/nv-rs/issues/353)) ([#369](https://github.com/JorisJonkers-dev/nv-rs/issues/369)) ([e6e97b2](https://github.com/JorisJonkers-dev/nv-rs/commit/e6e97b28a1978b11d102b53a9c4e21bcbdd9b44b))

## [0.8.0](https://github.com/JorisJonkers-dev/nv-rs/compare/v0.7.0...v0.8.0) (2026-11-18)


### Features

* **d3d9-wgpu:** sun shadows: characters cast (player in third person, people, creatures, bodies), one shadow each (slaterain/nv-rs[#726](https://github.com/JorisJonkers-dev/nv-rs/issues/726)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **engine-platform:** mods' files: list a folder the way the seam reads files (slaterain/nv-rs[#732](https://github.com/JorisJonkers-dev/nv-rs/issues/732)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **mod-archive:** mods' archives opened by the port itself: zip and 7z, on every system (slaterain/nv-rs[#722](https://github.com/JorisJonkers-dev/nv-rs/issues/722)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **mod-install:** prepares installed mods: their own folders and the game folder for mods, the same on every system (slaterain/nv-rs[#725](https://github.com/JorisJonkers-dev/nv-rs/issues/725)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** companion wheel switch, gun wobble, first-person slots, RemoveMe's refresh, CastImmediate (slaterain/nv-rs[#736](https://github.com/JorisJonkers-dev/nv-rs/issues/736)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** GetCalculatedWeaponDamage (slaterain/nv-rs[#756](https://github.com/JorisJonkers-dev/nv-rs/issues/756)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** GetFilesInFolder and GetFoldersInFolder through the mods' files seam (slaterain/nv-rs[#751](https://github.com/JorisJonkers-dev/nv-rs/issues/751)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** hit effects switch, item lists refreshed, and the bits and state its game patches read (9 commands) (slaterain/nv-rs[#729](https://github.com/JorisJonkers-dev/nv-rs/issues/729)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** inserted nodes and attached models on biped parts and add-ons; InsertNode and AttachModel registered (slaterain/nv-rs[#752](https://github.com/JorisJonkers-dev/nv-rs/issues/752)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** interface tiles and lines on models, texture swaps, the light at a point, character controllers reloaded (6 commands) (slaterain/nv-rs[#733](https://github.com/JorisJonkers-dev/nv-rs/issues/733)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** its game patch switches asked and changed in the game, and its set of marker forms (3 commands) (slaterain/nv-rs[#741](https://github.com/JorisJonkers-dev/nv-rs/issues/741)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** message boxes with a callback script (MessageBoxExAlt); SetFontFile built for G129 (slaterain/nv-rs[#731](https://github.com/JorisJonkers-dev/nv-rs/issues/731)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** ModelHasBlock, ReloadEquippedModels and RefreshAnimData; InsertNode and AttachModel built for the extender's model hooks (slaterain/nv-rs[#734](https://github.com/JorisJonkers-dev/nv-rs/issues/734)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** perks lists and removal, MoveAwayFrom, TravelToRef, excluded combat actions (slaterain/nv-rs[#748](https://github.com/JorisJonkers-dev/nv-rs/issues/748)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** RunScriptSnippet, RunBatchScript and Console on the script runner (slaterain/nv-rs[#743](https://github.com/JorisJonkers-dev/nv-rs/issues/743)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** SynchronizePosition and the HUD cursor (H13, H02), each registered with its patch (slaterain/nv-rs[#742](https://github.com/JorisJonkers-dev/nv-rs/issues/742)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** the built-in extender's script runner lists its folder through the mods' files seam (slaterain/nv-rs[#747](https://github.com/JorisJonkers-dev/nv-rs/issues/747)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** the built-in extender's script runner: mods' script files run at their moments (slaterain/nv-rs[#727](https://github.com/JorisJonkers-dev/nv-rs/issues/727)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** the muzzle flash patch FireWeaponEx makes on first use; FireWeaponEx registered (slaterain/nv-rs[#753](https://github.com/JorisJonkers-dev/nv-rs/issues/753)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** the task queue's tag 0x2B as the extender's queued command call, FireWeaponEx built for H01 (slaterain/nv-rs[#740](https://github.com/JorisJonkers-dev/nv-rs/issues/740)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **stewie:** a built-in tweaks plugin: its framework (each fix a row with its own off key) and its first crash fix (slaterain/nv-rs[#745](https://github.com/JorisJonkers-dev/nv-rs/issues/745)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **viewer:** play with mods on Linux and macOS: play.sh --mods (slaterain/nv-rs[#744](https://github.com/JorisJonkers-dev/nv-rs/issues/744)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))


### Bug Fixes

* **nvse:** AttachLine's and SwapTextureEx's game calls take the words the exe's functions take (slaterain/nv-rs[#746](https://github.com/JorisJonkers-dev/nv-rs/issues/746)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))
* **nvse:** the extender's ini from the port's folder first; teammate perks after a load; the killer cleared (slaterain/nv-rs[#728](https://github.com/JorisJonkers-dev/nv-rs/issues/728)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))


### Performance Improvements

* **engine-fallout-cell:** shorter hitch when cells unload: the clean-up for references that go away runs as native code (slaterain/nv-rs[#735](https://github.com/JorisJonkers-dev/nv-rs/issues/735)) ([e1b524d](https://github.com/JorisJonkers-dev/nv-rs/commit/e1b524dbfa5e7bfb37a25658ee51315df51f4131))

## [0.7.0](https://github.com/JorisJonkers-dev/nv-rs/compare/v0.6.0...v0.7.0) (2026-11-18)


### Features

* **d3d9-wgpu:** new lighting: the sun light comes from the sun and moon discs (bSunLightFromSky, on with the sun shadows) (slaterain/nv-rs[#678](https://github.com/JorisJonkers-dev/nv-rs/issues/678)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **d3d9-wgpu:** sun shadows: cables cast faint, soft shadows at their real weight (slaterain/nv-rs[#693](https://github.com/JorisJonkers-dev/nv-rs/issues/693)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **d3d9-wgpu:** sun shadows: distant shadows (land, hills and large buildings past 140 m, from one coarse far map) (slaterain/nv-rs[#715](https://github.com/JorisJonkers-dev/nv-rs/issues/715)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **d3d9-wgpu:** sun shadows: the noon shadow on the city's streets and the light at dusk and dawn (slaterain/nv-rs[#690](https://github.com/JorisJonkers-dev/nv-rs/issues/690)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-core:** Apple silicon: every quick-estimate helper on the shared model off x86 (built and unit tested; the start stays refused) (slaterain/nv-rs[#698](https://github.com/JorisJonkers-dev/nv-rs/issues/698)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-core:** Apple silicon: the processor's quick estimates (RCPSS, RSQRTSS) in one place, an exact model of a PC's off x86 (slaterain/nv-rs[#695](https://github.com/JorisJonkers-dev/nv-rs/issues/695)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-core:** Apple silicon: the x87 maths in software, exact where the FPU is; x86's indefinite NaN (slaterain/nv-rs[#706](https://github.com/JorisJonkers-dev/nv-rs/issues/706)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-platform:** Linux and macOS: sounds at other rates are heard (the resampler's tables from their formula); Windows unchanged (slaterain/nv-rs[#691](https://github.com/JorisJonkers-dev/nv-rs/issues/691)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-platform:** Linux and macOS: the game's code pages (1252, 1250, 1251) and Windows' case, from tables measured on Windows (slaterain/nv-rs[#684](https://github.com/JorisJonkers-dev/nv-rs/issues/684)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-platform:** Linux and macOS: the screen, the graphics card and the sound device answered from what the system reports (slaterain/nv-rs[#681](https://github.com/JorisJonkers-dev/nv-rs/issues/681)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-platform:** macOS: the game's window on the program's main thread (built and unit tested; Apple silicon still refused for the processor) (slaterain/nv-rs[#694](https://github.com/JorisJonkers-dev/nv-rs/issues/694)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fallout-gameplay:** the built-in extender's game patches GB part 6: fRepairSkillMax 10, Big Guns' name and derivation and switch (slaterain/nv-rs[#686](https://github.com/JorisJonkers-dev/nv-rs/issues/686)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fallout-gameplay:** with the built-in extender, its light data, flicker and pulse, light editor ids and lights tagged in meshes (GL part 2) (slaterain/nv-rs[#700](https://github.com/JorisJonkers-dev/nv-rs/issues/700)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fallout-gameplay:** with the built-in extender, loading screens made for a place shown on their own (bLoadScreenFix, GA part 7) (slaterain/nv-rs[#692](https://github.com/JorisJonkers-dev/nv-rs/issues/692)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fallout-gameplay:** with the built-in extender, magic effect fixes, Medicine or Survival scaling and chem durations (GL part 1) (slaterain/nv-rs[#689](https://github.com/JorisJonkers-dev/nv-rs/issues/689)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fallout-gameplay:** with the built-in extender, modded weapons read Name+ in every item list, Ctrl takes one and Shift takes all without the quantity menu (GB part 4) (slaterain/nv-rs[#683](https://github.com/JorisJonkers-dev/nv-rs/issues/683)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fallout-gameplay:** with the built-in extender, NPCs keep perks of their own (uNPCPerks part 1, GE part 3) (slaterain/nv-rs[#699](https://github.com/JorisJonkers-dev/nv-rs/issues/699)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fallout-gameplay:** with the built-in extender, NPCs show their weapon mods, random NPC weapon mods, save and global fixes, fresh mod logs, markers by id (GH2 part 4, GF part 3) (slaterain/nv-rs[#688](https://github.com/JorisJonkers-dev/nv-rs/issues/688)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fallout-gameplay:** with the built-in extender, uNPCPerks takes effect (GE part 4, O18 part 2) (slaterain/nv-rs[#709](https://github.com/JorisJonkers-dev/nv-rs/issues/709)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fallout-gameplay:** with the built-in extender's bEnableFO3Repair, Fallout 3 style repair capped by the Repair skill (GB part 5) (slaterain/nv-rs[#685](https://github.com/JorisJonkers-dev/nv-rs/issues/685)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fomod:** prepares installing FOMOD mods: the installer reader and file planner (slaterain/nv-rs[#720](https://github.com/JorisJonkers-dev/nv-rs/issues/720)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **host:** Apple silicon: the start goes on (builds and passes unit tests; maths differs from a PC's in the last bits in a few functions; never started with the game) (slaterain/nv-rs[#710](https://github.com/JorisJonkers-dev/nv-rs/issues/710)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **host:** Linux: the start is no longer refused (x86-64); built and unit tested there, not yet started with the game (slaterain/nv-rs[#687](https://github.com/JorisJonkers-dev/nv-rs/issues/687)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))


### Bug Fixes

* **d3d9-wgpu:** Linux and macOS: new games get past the intro movie on AMD, Intel and Apple cards (slaterain/nv-rs[#716](https://github.com/JorisJonkers-dev/nv-rs/issues/716)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **d3d9-wgpu:** Linux and macOS: the F11 counter and the loading card show their text (slaterain/nv-rs[#721](https://github.com/JorisJonkers-dev/nv-rs/issues/721)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **d3d9-wgpu:** rsq, log and nrm of 0 given explicitly; an HDR watch and target diagnostics for the AMD reports (slaterain/nv-rs[#724](https://github.com/JorisJonkers-dev/nv-rs/issues/724)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **d3d9-wgpu:** the sun shadows' own index lists carry the video-memory budget's charge ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **distribution:** Linux and macOS packages: the notice check reads only path crates' own folders; the menu configuration mod's notice listed (slaterain/nv-rs[#702](https://github.com/JorisJonkers-dev/nv-rs/issues/702)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-havok:** Apple silicon: Havok's tick counter answers off x86-64 instead of stopping the run (slaterain/nv-rs[#701](https://github.com/JorisJonkers-dev/nv-rs/issues/701)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-platform:** Linux and macOS: an unreadable time zone is UTC, the user name from USER, a renamed card said (slaterain/nv-rs[#723](https://github.com/JorisJonkers-dev/nv-rs/issues/723)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-platform:** Linux and macOS: listed code-page names that read as UTF-8 open; listings keep what they saw; mods' kept copies found without case (slaterain/nv-rs[#697](https://github.com/JorisJonkers-dev/nv-rs/issues/697)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-platform:** Linux and macOS: review follow-ups of slaterain/nv-rs[#697](https://github.com/JorisJonkers-dev/nv-rs/issues/697) for file names (slaterain/nv-rs[#714](https://github.com/JorisJonkers-dev/nv-rs/issues/714)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-platform:** Linux and macOS: the save menu no longer stops on saves made in the run (slaterain/nv-rs[#718](https://github.com/JorisJonkers-dev/nv-rs/issues/718)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-platform:** off Windows a game device wgpu cannot make ends the run; the merged machine table test names its code page ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **engine-platform:** off Windows the shader profile getters are upstream's; the soft-stop search test is Windows only ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fallout-gameplay:** NPC perks: no new version for writes that change nothing; burst reads under the lock (slaterain/nv-rs[#711](https://github.com/JorisJonkers-dev/nv-rs/issues/711)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **fork:** verify-sync.sh compares the file lists in the C locale ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **host:** Linux first start: the pre-order pack search no longer stops the game; mods' files folder; a sharp picture on a scaled desktop (slaterain/nv-rs[#712](https://github.com/JorisJonkers-dev/nv-rs/issues/712)) ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))
* **host:** the merged mods' files folder and first-start steps build and are formatted; a clippy allow on a constant assertion ([053461a](https://github.com/JorisJonkers-dev/nv-rs/commit/053461aca83212ce20fef24d3465a2d8f06a3ce8))

## [0.6.0](https://github.com/JorisJonkers-dev/nv-rs/compare/v0.5.0...v0.6.0) (2026-11-17)


### Features

* **d3d9-wgpu:** new lighting: ambient occlusion in third person too, a little stronger (0.7), fAmbientOcclusionStrength (slaterain/nv-rs[#669](https://github.com/JorisJonkers-dev/nv-rs/issues/669)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **d3d9-wgpu:** sun shadows on Vulkan, off by default: real shadows inside the game's lighting, following the game's sun light (slaterain/nv-rs[#668](https://github.com/JorisJonkers-dev/nv-rs/issues/668)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **d3d9-wgpu:** Vulkan: the intro movie shows its picture (draws without shaders through fixed-function shaders) (slaterain/nv-rs[#666](https://github.com/JorisJonkers-dev/nv-rs/issues/666)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **distribution:** Linux and macOS preview: the start script, the READ MEs and the packaging script (slaterain/nv-rs[#631](https://github.com/JorisJonkers-dev/nv-rs/issues/631)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **engine-platform:** Linux and macOS: the game's paths on drive Z: (Wine's way), names found without case, file listings with their times (slaterain/nv-rs[#633](https://github.com/JorisJonkers-dev/nv-rs/issues/633)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **engine-platform:** sound on the speakers on macOS through cpal (slaterain/nv-rs[#637](https://github.com/JorisJonkers-dev/nv-rs/issues/637)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **engine-platform:** the distortion effect on Linux and macOS from the DLL's 32 constants; Windows still measures and checks them (slaterain/nv-rs[#639](https://github.com/JorisJonkers-dev/nv-rs/issues/639)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **engine-platform:** the game's window, its picture and mouse and keyboard through winit off Windows (slaterain/nv-rs[#640](https://github.com/JorisJonkers-dev/nv-rs/issues/640)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **extenders:** mods can keep arrays, texts and logs in files (6; 1,011 of 1,118) (slaterain/nv-rs[#643](https://github.com/JorisJonkers-dev/nv-rs/issues/643)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **fallout-gameplay:** the built-in extender's combat formulas: weapon and armour condition, armour wear cap, no-movement combat (GC part 4) (slaterain/nv-rs[#653](https://github.com/JorisJonkers-dev/nv-rs/issues/653)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **fallout-gameplay:** the built-in extender's game patches GB part 3: crafting value, weight and ammo effects, barter tab title and vendor caps, quantity mouse wheel (slaterain/nv-rs[#679](https://github.com/JorisJonkers-dev/nv-rs/issues/679)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **fallout-gameplay:** with the built-in extender, door stealing by the door's own rules, SetScale clamped not rounded, weapon node cleared, clouds after leaving a building, water lookup (GH2 part 3) (slaterain/nv-rs[#680](https://github.com/JorisJonkers-dev/nv-rs/issues/680)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **fallout-gameplay:** with the built-in extender, immobile creatures stay up, fixed bodies follow their objects, rumble setting, jump height, mouse movement switch, sneak collision shape (GX) (slaterain/nv-rs[#671](https://github.com/JorisJonkers-dev/nv-rs/issues/671)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **fallout-gameplay:** with the built-in extender, sound pitch settings, shuffled region music, .ogg folder sounds, FalloutCustom.ini (GW part 3) (slaterain/nv-rs[#676](https://github.com/JorisJonkers-dev/nv-rs/issues/676)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **fallout-gameplay:** with the built-in extender, sun glare texture, high-noon light, wind speed setting, silenced shots for everyone, acoustic space fix, forced climate (GW part 1) (slaterain/nv-rs[#673](https://github.com/JorisJonkers-dev/nv-rs/issues/673)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **fallout-gameplay:** with the built-in extender, the game's corner messages last the default message time, mods' keep theirs (GA part 6, G09) (slaterain/nv-rs[#670](https://github.com/JorisJonkers-dev/nv-rs/issues/670)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **fallout-gameplay:** with the built-in extender, V.A.T.S. shows the damage threshold its hit damage used (GA part 5, G121) (slaterain/nv-rs[#667](https://github.com/JorisJonkers-dev/nv-rs/issues/667)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **host:** Linux and macOS: finding the game and the Proton saves; the save guard works there (slaterain/nv-rs[#630](https://github.com/JorisJonkers-dev/nv-rs/issues/630)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))
* **host:** Windows is told the game is not background work (on by default; bNotBackground=0 turns it off) (slaterain/nv-rs[#663](https://github.com/JorisJonkers-dev/nv-rs/issues/663)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))


### Bug Fixes

* **fallout-gameplay:** with the built-in extender, GameDaysPassed no longer drifts (GW part 2, O5 bGameDaysPassedFix) (slaterain/nv-rs[#674](https://github.com/JorisJonkers-dev/nv-rs/issues/674)) ([201d396](https://github.com/JorisJonkers-dev/nv-rs/commit/201d396a138638aa439a7ba76bdcdb1081901a0f))

## [0.5.0](https://github.com/JorisJonkers-dev/nv-rs/compare/v0.4.0...v0.5.0) (2026-11-17)


### Features

* **bench:** a memory guard on every run: host and GPU caps, a session check and a halt ([#341](https://github.com/JorisJonkers-dev/nv-rs/issues/341)) ([66955c1](https://github.com/JorisJonkers-dev/nv-rs/commit/66955c1006d6e9cca3e19d4893437c8cceff8ce9))
* **bench:** unlimited by default, wall-clock real-input drivers, the quick suite and a headline suite table ([#184](https://github.com/JorisJonkers-dev/nv-rs/issues/184)) ([438736d](https://github.com/JorisJonkers-dev/nv-rs/commit/438736d8921dd2538e0fbdc0c69644e0af937ba8))
* **bench:** watch a running nested session, record it, and replay it with a frame-time timeline ([#116](https://github.com/JorisJonkers-dev/nv-rs/issues/116)) ([ecad7dc](https://github.com/JorisJonkers-dev/nv-rs/commit/ecad7dcde513476ae09f017b93883ccfa4445f4c))
* **ci:** weekly upstream sync, a warn-only divergence report and a changed-line coverage gate ([#117](https://github.com/JorisJonkers-dev/nv-rs/issues/117)) ([3b2bf65](https://github.com/JorisJonkers-dev/nv-rs/commit/3b2bf6579e60efaa198e12a57ef81e21fb4f0e8b))
* **d3d9-replay:** a throwaway raw Vulkan prototype of the draw path for the replay-bench ([#185](https://github.com/JorisJonkers-dev/nv-rs/issues/185)) ([49ce914](https://github.com/JorisJonkers-dev/nv-rs/commit/49ce914819b0a6cdc4a2b25f8a7ae63548f09314))
* **d3d9-replay:** replay-bench of the recorded Direct3D 9 stream with a null floor and the wgpu backend ([#109](https://github.com/JorisJonkers-dev/nv-rs/issues/109)) ([34d3ee4](https://github.com/JorisJonkers-dev/nv-rs/commit/34d3ee4c868742dc540dc50509e10d0f60289218))
* **d3d9-wgpu:** a content-addressed shader cache, and the pipeline cache and list kept on Linux by default ([#104](https://github.com/JorisJonkers-dev/nv-rs/issues/104)) ([6273d73](https://github.com/JorisJonkers-dev/nv-rs/commit/6273d73e1c420e2fb11b67ab7d49b1550350c562))
* **d3d9-wgpu:** ambient occlusion on Vulkan, off by default (slaterain/nv-rs[#661](https://github.com/JorisJonkers-dev/nv-rs/issues/661)) ([b5ba0c8](https://github.com/JorisJonkers-dev/nv-rs/commit/b5ba0c835deffbb23b07975ba1fd201858503d38))
* **d3d9-wgpu:** NV_PERF reports the busy share of every thread of the process ([#100](https://github.com/JorisJonkers-dev/nv-rs/issues/100)) ([4657e92](https://github.com/JorisJonkers-dev/nv-rs/commit/4657e928a2de63331a7d78922cc3c49f22bf8d9c))
* **d3d9-wgpu:** NV_PIPELINE_CACHE_MAX_MB caps the pipeline folder and the recorded pipeline list ([#115](https://github.com/JorisJonkers-dev/nv-rs/issues/115)) ([3e1fe89](https://github.com/JorisJonkers-dev/nv-rs/commit/3e1fe89004c26bb6a617c962298bc411017292de))
* **d3d9-wgpu:** precompile the shaders of a load order into the caches ([#106](https://github.com/JorisJonkers-dev/nv-rs/issues/106)) ([aed8afd](https://github.com/JorisJonkers-dev/nv-rs/commit/aed8afdcb09ce024afbea92cc50290e0cb514ba2))
* **dev:** nv-rs-dev logs, a short report of a run from its logs ([#110](https://github.com/JorisJonkers-dev/nv-rs/issues/110)) ([56c980d](https://github.com/JorisJonkers-dev/nv-rs/commit/56c980ddfbfa35bea65e63b65313a920c5c77709))
* **engine-platform:** the Direct3D 9 layer builds on every system and the Win32 window moves into its own file (slaterain/nv-rs[#635](https://github.com/JorisJonkers-dev/nv-rs/issues/635)) ([63aacd4](https://github.com/JorisJonkers-dev/nv-rs/commit/63aacd483990cb45945651221e968dd638da1518))
* **engine-platform:** the log says where the game's window went, and when a play run runs as a test (slaterain/nv-rs[#627](https://github.com/JorisJonkers-dev/nv-rs/issues/627)) ([8e90f73](https://github.com/JorisJonkers-dev/nv-rs/commit/8e90f73dbe4b0696cf988b981e7ff5f04ba9ab93))
* **engine-platform:** the machine's answers off Windows from what the system reports (slaterain/nv-rs[#645](https://github.com/JorisJonkers-dev/nv-rs/issues/645)) ([b5ba0c8](https://github.com/JorisJonkers-dev/nv-rs/commit/b5ba0c835deffbb23b07975ba1fd201858503d38))
* **engine-typed:** the geometry family on by default after its gate (slaterain/nv-rs[#654](https://github.com/JorisJonkers-dev/nv-rs/issues/654)) ([b5ba0c8](https://github.com/JorisJonkers-dev/nv-rs/commit/b5ba0c835deffbb23b07975ba1fd201858503d38))
* **extenders:** a mod settings menu's plugin built in, off by default (slaterain/nv-rs[#644](https://github.com/JorisJonkers-dev/nv-rs/issues/644)) ([b5ba0c8](https://github.com/JorisJonkers-dev/nv-rs/commit/b5ba0c835deffbb23b07975ba1fd201858503d38))
* **extenders:** five more built-in extender commands, package C3 (1003 of 1,118) (slaterain/nv-rs[#646](https://github.com/JorisJonkers-dev/nv-rs/issues/646)) ([acbf8df](https://github.com/JorisJonkers-dev/nv-rs/commit/acbf8df7d739847fbad6bd870a1bd8abc0812652))
* **extenders:** mods can read and keep their own settings files (1,005 of 1,118) (slaterain/nv-rs[#642](https://github.com/JorisJonkers-dev/nv-rs/issues/642)) ([b5ba0c8](https://github.com/JorisJonkers-dev/nv-rs/commit/b5ba0c835deffbb23b07975ba1fd201858503d38))
* **extenders:** the built-in extender plugin's "installed" answer becomes a run-time switch, off by default (slaterain/nv-rs[#628](https://github.com/JorisJonkers-dev/nv-rs/issues/628)) ([8e90f73](https://github.com/JorisJonkers-dev/nv-rs/commit/8e90f73dbe4b0696cf988b981e7ff5f04ba9ab93))
* **extenders:** the built-in extender's ar_Cat command (1,002 of 1,118) (slaterain/nv-rs[#650](https://github.com/JorisJonkers-dev/nv-rs/issues/650)) ([acbf8df](https://github.com/JorisJonkers-dev/nv-rs/commit/acbf8df7d739847fbad6bd870a1bd8abc0812652))
* **extenders:** the built-in extender's own extra data on references and inventory stacks (997 of 1,118) (slaterain/nv-rs[#619](https://github.com/JorisJonkers-dev/nv-rs/issues/619)) ([acbf8df](https://github.com/JorisJonkers-dev/nv-rs/commit/acbf8df7d739847fbad6bd870a1bd8abc0812652))
* **extenders:** the built-in UI Organizer carries its own list of the UI mods it knows (slaterain/nv-rs[#655](https://github.com/JorisJonkers-dev/nv-rs/issues/655)) ([b5ba0c8](https://github.com/JorisJonkers-dev/nv-rs/commit/b5ba0c835deffbb23b07975ba1fd201858503d38))
* **extenders:** three more built-in extender commands, package C2c (998 of 1,118) (slaterain/nv-rs[#641](https://github.com/JorisJonkers-dev/nv-rs/issues/641)) ([63aacd4](https://github.com/JorisJonkers-dev/nv-rs/commit/63aacd483990cb45945651221e968dd638da1518))
* **extenders:** UI Organizer says in the log what it read, added and left out (slaterain/nv-rs[#638](https://github.com/JorisJonkers-dev/nv-rs/issues/638)) ([b5ba0c8](https://github.com/JorisJonkers-dev/nv-rs/commit/b5ba0c835deffbb23b07975ba1fd201858503d38))
* **fallout-gameplay:** five more of the built-in extender's game patches (group GD, part 3) (slaterain/nv-rs[#624](https://github.com/JorisJonkers-dev/nv-rs/issues/624)) ([8e90f73](https://github.com/JorisJonkers-dev/nv-rs/commit/8e90f73dbe4b0696cf988b981e7ff5f04ba9ab93))
* **fallout-gameplay:** the built-in extender's corner messages up to 515 characters, group GA part 3 (slaterain/nv-rs[#625](https://github.com/JorisJonkers-dev/nv-rs/issues/625)) ([63aacd4](https://github.com/JorisJonkers-dev/nv-rs/commit/63aacd483990cb45945651221e968dd638da1518))
* **fallout-gameplay:** the built-in extender's game patches, group GA part 4 (slaterain/nv-rs[#656](https://github.com/JorisJonkers-dev/nv-rs/issues/656)) ([b5ba0c8](https://github.com/JorisJonkers-dev/nv-rs/commit/b5ba0c835deffbb23b07975ba1fd201858503d38))
* **fallout-gameplay:** the built-in extender's game patches, group GB part 2 (slaterain/nv-rs[#623](https://github.com/JorisJonkers-dev/nv-rs/issues/623)) ([acbf8df](https://github.com/JorisJonkers-dev/nv-rs/commit/acbf8df7d739847fbad6bd870a1bd8abc0812652))
* **fallout-gameplay:** the built-in extender's game patches, group GD part 4 (slaterain/nv-rs[#648](https://github.com/JorisJonkers-dev/nv-rs/issues/648)) ([acbf8df](https://github.com/JorisJonkers-dev/nv-rs/commit/acbf8df7d739847fbad6bd870a1bd8abc0812652))
* **fallout-gameplay:** the built-in extender's game patches, group GF part 1 (slaterain/nv-rs[#622](https://github.com/JorisJonkers-dev/nv-rs/issues/622)) ([63aacd4](https://github.com/JorisJonkers-dev/nv-rs/commit/63aacd483990cb45945651221e968dd638da1518))
* **fallout-gameplay:** the built-in extender's game patches, group GF part 2 (slaterain/nv-rs[#649](https://github.com/JorisJonkers-dev/nv-rs/issues/649)) ([acbf8df](https://github.com/JorisJonkers-dev/nv-rs/commit/acbf8df7d739847fbad6bd870a1bd8abc0812652))
* **fallout-gameplay:** the built-in extender's game patches, group GS part 4 (slaterain/nv-rs[#636](https://github.com/JorisJonkers-dev/nv-rs/issues/636)) ([acbf8df](https://github.com/JorisJonkers-dev/nv-rs/commit/acbf8df7d739847fbad6bd870a1bd8abc0812652))
* **fallout-gameplay:** the built-in extender's hit damage and ammo scripts on impact, group GC part 3 (slaterain/nv-rs[#621](https://github.com/JorisJonkers-dev/nv-rs/issues/621)) ([63aacd4](https://github.com/JorisJonkers-dev/nv-rs/commit/63aacd483990cb45945651221e968dd638da1518))
* **hitch:** a flight recorder for hitches in real play, a per-run summary with flags, and nv-rs-dev hitches and runs ([#105](https://github.com/JorisJonkers-dev/nv-rs/issues/105)) ([9b6505f](https://github.com/JorisJonkers-dev/nv-rs/commit/9b6505f447bb60c9999631ba999910dea079adde))
* **hitch:** feed the recorder from platform calls, locks, files, sound and the wgpu backend; bench reports hitches ([#112](https://github.com/JorisJonkers-dev/nv-rs/issues/112)) ([7224600](https://github.com/JorisJonkers-dev/nv-rs/commit/7224600eacb503edfd320c06a55a7945fb6a11a0))
* **host:** NV_FPS_LIMIT, the plain name for the play frame limit; docs say play is unlimited by default ([#107](https://github.com/JorisJonkers-dev/nv-rs/issues/107)) ([1475126](https://github.com/JorisJonkers-dev/nv-rs/commit/1475126609771044eedc69c21989eee031b0a26d))
* **host:** script extenders' own settings files kept between sessions beside the saves (slaterain/nv-rs[#629](https://github.com/JorisJonkers-dev/nv-rs/issues/629)) ([acbf8df](https://github.com/JorisJonkers-dev/nv-rs/commit/acbf8df7d739847fbad6bd870a1bd8abc0812652))
* **host:** the play build compiles on Linux and macOS; starting the game on macOS is refused with a clear message ([e80e8b1](https://github.com/JorisJonkers-dev/nv-rs/commit/e80e8b12e4730c5545419af1f00de4ecff217a4c))
* **log:** a quiet terminal by default, a plain-text log beside the JSON log, NV_LOG_TERM ([#108](https://github.com/JorisJonkers-dev/nv-rs/issues/108)) ([d5b9186](https://github.com/JorisJonkers-dev/nv-rs/commit/d5b91861aecb85e99a86cec98ae152d1dd166664))
* **log:** the Direct3D layer's and D3DX's messages follow the terminal policy ([#113](https://github.com/JorisJonkers-dev/nv-rs/issues/113)) ([11ceae4](https://github.com/JorisJonkers-dev/nv-rs/commit/11ceae49436f3144310029b595e2544ba1fe3c49))


### Bug Fixes

* **bench:** a step counts as walking only past 30 units, and the first room checks again before its walks ([#338](https://github.com/JorisJonkers-dev/nv-rs/issues/338)) ([9bd462d](https://github.com/JorisJonkers-dev/nv-rs/commit/9bd462d771b25d3be5991325ab37e863ff5b447f))
* **bench:** free the player from the opening's package, keep the quick suite under 120 s, and add a sneak check ([#325](https://github.com/JorisJonkers-dev/nv-rs/issues/325)) ([8b3649d](https://github.com/JorisJonkers-dev/nv-rs/commit/8b3649d2371a65743fe9bc9d8c5c5a3a87f20b55))
* **bench:** lock waiters are served in the order they came, and bad window options are refused before waiting ([#111](https://github.com/JorisJonkers-dev/nv-rs/issues/111)) ([67b5d96](https://github.com/JorisJonkers-dev/nv-rs/commit/67b5d9676c3eb6c4b0a24f8576792e66d966dc3f))
* **bench:** quick suite start: no second warm-up in the first room, a walk a box stopped is made again ([#347](https://github.com/JorisJonkers-dev/nv-rs/issues/347)) ([b4266c6](https://github.com/JorisJonkers-dev/nv-rs/commit/b4266c6141eeec5298ee8aeaf8941f32f351c383))
* **d3d9-wgpu:** log the pipeline cache eviction through fork_log ([#316](https://github.com/JorisJonkers-dev/nv-rs/issues/316)) ([09f4211](https://github.com/JorisJonkers-dev/nv-rs/commit/09f4211024b832e661c8766e88d3f5d852509dd8))
* **dev:** the logs tool's fixtures write %USERPROFILE% instead of a user-profile path ([#186](https://github.com/JorisJonkers-dev/nv-rs/issues/186)) ([311b645](https://github.com/JorisJonkers-dev/nv-rs/commit/311b64589bb9bab34a57e9e0408f776fdc72d8ed))
* **engine-platform:** keep no 8.3 short names off Windows ([#332](https://github.com/JorisJonkers-dev/nv-rs/issues/332)) ([4ee9b1f](https://github.com/JorisJonkers-dev/nv-rs/commit/4ee9b1fbaa06561d5a68ca91601b7323c6e1f76c))
* **engine-platform:** keep the argument-word trait on every target after the replay of [#617](https://github.com/JorisJonkers-dev/nv-rs/issues/617) ([7ca8070](https://github.com/JorisJonkers-dev/nv-rs/commit/7ca8070fcacbd79e4dded16b6f8be3e786e74eee))
* **engine-platform:** mods' own files found without regard to case in the port's folder off Windows ([acbf8df](https://github.com/JorisJonkers-dev/nv-rs/commit/acbf8df7d739847fbad6bd870a1bd8abc0812652))
* **fallout-gameplay:** with the built-in extender, new characters and creatures start without leftover flags (slaterain/nv-rs[#662](https://github.com/JorisJonkers-dev/nv-rs/issues/662)) ([b5ba0c8](https://github.com/JorisJonkers-dev/nv-rs/commit/b5ba0c835deffbb23b07975ba1fd201858503d38))
* **hitch:** loads are load time, not hitches; only steady play counts toward the rate ([#339](https://github.com/JorisJonkers-dev/nv-rs/issues/339)) ([c91dbf9](https://github.com/JorisJonkers-dev/nv-rs/commit/c91dbf9805f3eb00a4099a1031f5571701cc05b7))
* **hitch:** screen switches are load time, the event ring keeps them, and the quick suite starts sooner ([#345](https://github.com/JorisJonkers-dev/nv-rs/issues/345)) ([2723e0e](https://github.com/JorisJonkers-dev/nv-rs/commit/2723e0e2672870dc7ae493da169636934d78647c))
* **host:** NV_SAVE_OUT takes a folder off Windows ([#335](https://github.com/JorisJonkers-dev/nv-rs/issues/335)) ([34f3d44](https://github.com/JorisJonkers-dev/nv-rs/commit/34f3d442d0185efb847374078d2869325055073a))
* **host:** the machine table answers without a usable profile, and the kept table is the whole one ([b5ba0c8](https://github.com/JorisJonkers-dev/nv-rs/commit/b5ba0c835deffbb23b07975ba1fd201858503d38))
* **package:** the research folder's name counts only as a path in the package check (slaterain/nv-rs[#658](https://github.com/JorisJonkers-dev/nv-rs/issues/658)) ([b5ba0c8](https://github.com/JorisJonkers-dev/nv-rs/commit/b5ba0c835deffbb23b07975ba1fd201858503d38))
* **platform:** keep the game's Settings menu choices between starts ([#346](https://github.com/JorisJonkers-dev/nv-rs/issues/346)) ([121ead1](https://github.com/JorisJonkers-dev/nv-rs/commit/121ead1e3b57ad889d0919e41504c2e9384f66a8))


### Performance Improvements

* **engine-platform:** ReadFile reads the host file outside the kernel lock ([#102](https://github.com/JorisJonkers-dev/nv-rs/issues/102)) ([01a4224](https://github.com/JorisJonkers-dev/nv-rs/commit/01a422480f6cabfc86f3814c1889491846c43c72))

## [0.4.0](https://github.com/JorisJonkers-dev/nv-rs/compare/v0.3.0...v0.4.0) (2026-11-16)


### Features

* **bench:** a machine-wide lock for bench runs, and runs that shared the machine are marked noisy ([#101](https://github.com/JorisJonkers-dev/nv-rs/issues/101)) ([133b81d](https://github.com/JorisJonkers-dev/nv-rs/commit/133b81d2c4d226cf2bf419cf1f2c8d1c3c45e049))
* **bench:** chargen scenario and a play-frame script clock ([#81](https://github.com/JorisJonkers-dev/nv-rs/issues/81)) ([42b195d](https://github.com/JorisJonkers-dev/nv-rs/commit/42b195d3ee402ec15fe651211f931ba1e94b10ee))
* **bench:** nested headless window path, the game's sound clock into a null sink, and world scenarios ([#99](https://github.com/JorisJonkers-dev/nv-rs/issues/99)) ([d2d0ef3](https://github.com/JorisJonkers-dev/nv-rs/commit/d2d0ef3ead28a3befc9b460d9a6d792a10e74e09))
* **d3d9-wgpu:** NV_PERF, a per-frame CPU timeline of the backend with a spike report ([#97](https://github.com/JorisJonkers-dev/nv-rs/issues/97)) ([55bd979](https://github.com/JorisJonkers-dev/nv-rs/commit/55bd979aa19ae69c77e5c419b3cf3cae6932c878))
* **dev:** local build, run, smoke, test and check loop ([#68](https://github.com/JorisJonkers-dev/nv-rs/issues/68)) ([527f766](https://github.com/JorisJonkers-dev/nv-rs/commit/527f766ff86e69b2fdd23a4f875c97887c359b74))
* **dev:** nv-rs-dev affected and ci-lite run CI's crate selection locally ([#75](https://github.com/JorisJonkers-dev/nv-rs/issues/75)) ([80ff2be](https://github.com/JorisJonkers-dev/nv-rs/commit/80ff2beeda5b9bf1b439312362e4730feb0f69a3))
* **dev:** nv-rs-dev bench, scripted playthrough runs with frame-time, memory and stability reports ([#79](https://github.com/JorisJonkers-dev/nv-rs/issues/79)) ([0c5cb9d](https://github.com/JorisJonkers-dev/nv-rs/commit/0c5cb9da614e3dd162659c071103e501c3a9ccee))
* **dev:** nv-rs-dev build --opt, and bench runs the optimised play-opt build by default ([#98](https://github.com/JorisJonkers-dev/nv-rs/issues/98)) ([223d88d](https://github.com/JorisJonkers-dev/nv-rs/commit/223d88d624fcbbf4fa58c27aeda1b74268a3b4ad))
* **dev:** nv-rs-dev matrix, compare and clean, and a timed cold build ([#94](https://github.com/JorisJonkers-dev/nv-rs/issues/94)) ([717730a](https://github.com/JorisJonkers-dev/nv-rs/commit/717730a3c4baa19a256a40265a94fadb5959f486))
* **fork:** desktop-hint.sh names the focused output per compositor for NV_MONITOR ([#72](https://github.com/JorisJonkers-dev/nv-rs/issues/72)) ([fc531b9](https://github.com/JorisJonkers-dev/nv-rs/commit/fc531b9e0845e6f3c669aee47e49b0005cbe181f))
* **fork:** NV_MEMLOG memory timeline of guest heap, host RSS and GPU counters ([#70](https://github.com/JorisJonkers-dev/nv-rs/issues/70)) ([c8c8165](https://github.com/JorisJonkers-dev/nv-rs/commit/c8c8165970f9213505d2c681718988d36d5875eb))
* **host:** keep saves in the port's own Saves folder on Linux, synced to disk ([#90](https://github.com/JorisJonkers-dev/nv-rs/issues/90)) ([afa09e1](https://github.com/JorisJonkers-dev/nv-rs/commit/afa09e1219e0c2a43613afd673523f7f7202d347))
* **linux:** the game's sound output on PipeWire and PulseAudio, with a null sink fallback ([#55](https://github.com/JorisJonkers-dev/nv-rs/issues/55)) ([acd99da](https://github.com/JorisJonkers-dev/nv-rs/commit/acd99da4e7adb0edd96eaa3dc7b6b1ef440d82d7))
* **linux:** the game's window and rendering match the display it started on ([#57](https://github.com/JorisJonkers-dev/nv-rs/issues/57)) ([289baac](https://github.com/JorisJonkers-dev/nv-rs/commit/289baac843a7a55a09e5de970042d8ac91eec727))
* **linux:** the game's window is a real window and its keys, mouse and close reach the game ([#53](https://github.com/JorisJonkers-dev/nv-rs/issues/53)) ([250d03c](https://github.com/JorisJonkers-dev/nv-rs/commit/250d03c2a4c1a9321695526718d3372128298aa0))
* **log:** fork-log crate with levels, a JSON-lines file, stop and panic records and the soft-stop policy ([#84](https://github.com/JorisJonkers-dev/nv-rs/issues/84)) ([9a7bb60](https://github.com/JorisJonkers-dev/nv-rs/commit/9a7bb60fd4dab310573f7f77c1dda88eafb3d51e))
* **log:** game event trace for movies, input, audio, render targets, presents and the loading screen ([#88](https://github.com/JorisJonkers-dev/nv-rs/issues/88)) ([2ae74f7](https://github.com/JorisJonkers-dev/nv-rs/commit/2ae74f788e68c5b778dcd2f74d5831582ea47fd3))
* **log:** levels for the window, d3d9, sound and host messages; soft stops for BinkOpen, queries and DirectSound ([#87](https://github.com/JorisJonkers-dev/nv-rs/issues/87)) ([865b9d8](https://github.com/JorisJonkers-dev/nv-rs/commit/865b9d8336f97289b5849acf5bdcf7ca4c8aa334))
* **sound:** multichannel layout probe, explicit channel map and underrun reports ([#64](https://github.com/JorisJonkers-dev/nv-rs/issues/64)) ([832d2ea](https://github.com/JorisJonkers-dev/nv-rs/commit/832d2eaf4cde4ddacfab844a028d9d561c1f338e))
* **viewer:** Discord Rich Presence behind a default feature ([#59](https://github.com/JorisJonkers-dev/nv-rs/issues/59)) ([308d8b4](https://github.com/JorisJonkers-dev/nv-rs/commit/308d8b4cdf007b261a326951934e5c4de78094ca))
* **window:** idle inhibit, a close that always works, window report; DESKTOPS.md ([#76](https://github.com/JorisJonkers-dev/nv-rs/issues/76)) ([f6e3608](https://github.com/JorisJonkers-dev/nv-rs/commit/f6e3608968739f114260fca1a76ad29c8676c15d))


### Bug Fixes

* **coverage:** a missing plan artifact means nothing to measure, not a crash ([#95](https://github.com/JorisJonkers-dev/nv-rs/issues/95)) ([4d2ac5b](https://github.com/JorisJonkers-dev/nv-rs/commit/4d2ac5b2b4dfee613ca6cd3f23432784298c1958))
* **d3d9-wgpu:** a video-memory budget, a size clamp and memory reports so a runaway game stops before the driver runs out of memory ([#77](https://github.com/JorisJonkers-dev/nv-rs/issues/77)) ([dd79358](https://github.com/JorisJonkers-dev/nv-rs/commit/dd79358c620ef17f42b10b4a3c7be49ab170154f))
* **d3d9-wgpu:** draw the pre-transformed quad the Bink movie player uses without shaders ([#62](https://github.com/JorisJonkers-dev/nv-rs/issues/62)) ([77409ed](https://github.com/JorisJonkers-dev/nv-rs/commit/77409edd70868b77e00877c22b737a2810368a6e))
* **d3d9-wgpu:** movie tiles drew one flat colour (wrong usage bits in the built-in shaders) ([#89](https://github.com/JorisJonkers-dev/nv-rs/issues/89)) ([b138485](https://github.com/JorisJonkers-dev/nv-rs/commit/b138485df13dc0e62d2acc03cc94604533783d49))
* **d3d9:** machine table lines for event and occlusion queries on Linux ([#80](https://github.com/JorisJonkers-dev/nv-rs/issues/80)) ([4260a42](https://github.com/JorisJonkers-dev/nv-rs/commit/4260a42f2dde34db806555efa889c37137b4c801))
* **fork:** add the uio crate to the release-files lock file ([#58](https://github.com/JorisJonkers-dev/nv-rs/issues/58)) ([61692b6](https://github.com/JorisJonkers-dev/nv-rs/commit/61692b678fd5f6c4d665afe1cfcbc6fc5f2ecace))
* **linux:** alt-tab back redraws, skipped frames do not grow the queue, a lost device ends the run ([#61](https://github.com/JorisJonkers-dev/nv-rs/issues/61)) ([f7ab726](https://github.com/JorisJonkers-dev/nv-rs/commit/f7ab7263186bfa27be69ddbc148fb44ed1bad0ea))
* **linux:** first-run settings, Z: folders and host path mapping so the game gets past frame 0 ([#51](https://github.com/JorisJonkers-dev/nv-rs/issues/51)) ([a5f7044](https://github.com/JorisJonkers-dev/nv-rs/commit/a5f70443705bb2d0517303259b236384d49c4a38))
* **linux:** hold the pointer in the window and scale its motion to the game's screen ([#56](https://github.com/JorisJonkers-dev/nv-rs/issues/56)) ([8e2d619](https://github.com/JorisJonkers-dev/nv-rs/commit/8e2d619d0c9455ee4a8aecdc38518a8a2b630cf2))
* **linux:** one event loop for the monitor query and the window; NV_RESOLUTION ([#66](https://github.com/JorisJonkers-dev/nv-rs/issues/66)) ([fff7507](https://github.com/JorisJonkers-dev/nv-rs/commit/fff75070701c7e28cb8417a78b9cfdfcd927253f))
* **linux:** the compositor places the game's window on the current output (Wayland) ([#69](https://github.com/JorisJonkers-dev/nv-rs/issues/69)) ([584935a](https://github.com/JorisJonkers-dev/nv-rs/commit/584935aba281856c80ef06d0f2e165619f5ba863))
* **linux:** the game's folder is a Z: path so relative Windows paths resolve ([#54](https://github.com/JorisJonkers-dev/nv-rs/issues/54)) ([895e5ab](https://github.com/JorisJonkers-dev/nv-rs/commit/895e5abbab834f5a2410941cf9739831a5ec10c8))
* **linux:** the generated device caps take textures of any size ([#67](https://github.com/JorisJonkers-dev/nv-rs/issues/67)) ([ef774a0](https://github.com/JorisJonkers-dev/nv-rs/commit/ef774a0c5689db873956b3660ebf49e52af02138))
* **platform:** never stop a file call for a missing 8.3 short name ([#85](https://github.com/JorisJonkers-dev/nv-rs/issues/85)) ([970425a](https://github.com/JorisJonkers-dev/nv-rs/commit/970425a48bdc5d1e5567f92853d4889be5e410ea))
* **platform:** NTFS-like 8.3 short names for saves on Linux ([#71](https://github.com/JorisJonkers-dev/nv-rs/issues/71)) ([52604c6](https://github.com/JorisJonkers-dev/nv-rs/commit/52604c64c354bd80acb89f0930c7c5101b964eaf))
* **platform:** soft stops for the file calls that halted the game ([#93](https://github.com/JorisJonkers-dev/nv-rs/issues/93)) ([a9901f5](https://github.com/JorisJonkers-dev/nv-rs/commit/a9901f53e02bf97778e65642c81f3dce761256ee))
* **release-files:** stand in for the game patches' wraps too ([#82](https://github.com/JorisJonkers-dev/nv-rs/issues/82)) ([52a7396](https://github.com/JorisJonkers-dev/nv-rs/commit/52a73969e3703aec0c2557dd145ded31d464ea6c))
* **window:** a key tap shorter than a frame reaches the game; modifier keys are logged ([#96](https://github.com/JorisJonkers-dev/nv-rs/issues/96)) ([84be097](https://github.com/JorisJonkers-dev/nv-rs/commit/84be0972bd99566bba0701d76ce211bbda99c85e))


### Performance Improvements

* **ci:** shard the typed shader family across three jobs ([#73](https://github.com/JorisJonkers-dev/nv-rs/issues/73)) ([902c818](https://github.com/JorisJonkers-dev/nv-rs/commit/902c818c352ef18e314d1bca701696d044892969))

## [0.3.0](https://github.com/JorisJonkers-dev/nv-rs/compare/v0.2.0...v0.3.0) (2026-11-15)


### Features

* **linux:** a window layer for X11 and Wayland: winit window thread, handle registry, wgpu surface and DirectInput-shaped input ([#40](https://github.com/JorisJonkers-dev/nv-rs/issues/40)) ([2290958](https://github.com/JorisJonkers-dev/nv-rs/commit/22909582f13d450b48c3ad2ac504ddd272c61d8d))
* **linux:** machine tables for a Linux machine (adapter table, no-device sound table) ([#48](https://github.com/JorisJonkers-dev/nv-rs/issues/48)) ([79cd59b](https://github.com/JorisJonkers-dev/nv-rs/commit/79cd59bfac62604b4e3162762c9e0ec92df1c1f5))
* **linux:** pure-Rust ID3DXConstantTable readers and device-caps shader profiles ([#43](https://github.com/JorisJonkers-dev/nv-rs/issues/43)) ([ebe543d](https://github.com/JorisJonkers-dev/nv-rs/commit/ebe543d6b8597db7aa72b0252b9605566541c99e))
* **linux:** the Direct3D 9 recorder builds off Windows and draws a frame headless on wgpu ([#50](https://github.com/JorisJonkers-dev/nv-rs/issues/50)) ([23e8aa0](https://github.com/JorisJonkers-dev/nv-rs/commit/23e8aa07bf7be65534f7766b8fd67baf8944dd5c))


### Bug Fixes

* **ci:** package script splat and called-run detection in the Windows package workflow ([#44](https://github.com/JorisJonkers-dev/nv-rs/issues/44)) ([9bda5ff](https://github.com/JorisJonkers-dev/nv-rs/commit/9bda5ffe6c7837c259307bf92c7b0e2bc6a2efe0))

## [0.2.0](https://github.com/JorisJonkers-dev/nv-rs/compare/v0.1.0...v0.2.0) (2026-11-15)


### Features

* **fork:** release manifest and capabilities files for the release build ([#18](https://github.com/JorisJonkers-dev/nv-rs/issues/18)) ([898aa0d](https://github.com/JorisJonkers-dev/nv-rs/commit/898aa0deb819081e83900958399ecdb621f1f53e))
* **fork:** verify the sync in the Pipeline, report a blocked sync, keep fork-owned paths out of the replay ([#22](https://github.com/JorisJonkers-dev/nv-rs/issues/22)) ([4872cbd](https://github.com/JorisJonkers-dev/nv-rs/commit/4872cbd3634d9bbe0646843a3d02e0833d5dfb40))
* **fork:** Windows portable ZIP, unsigned installer and smoke test in the fork's CI ([#27](https://github.com/JorisJonkers-dev/nv-rs/issues/27)) ([c8a66f9](https://github.com/JorisJonkers-dev/nv-rs/commit/c8a66f97f2bb5faf68b51a5ec35994384d43b50a))
* **linux:** answer file times, known folders, memory status, the clock start and table paths off Windows ([#32](https://github.com/JorisJonkers-dev/nv-rs/issues/32)) ([35e93f9](https://github.com/JorisJonkers-dev/nv-rs/commit/35e93f9e9d249b7f254588472667fc15876e73df))
* **linux:** libvkd3d-shader backend, the first working shader compiler off Windows ([#39](https://github.com/JorisJonkers-dev/nv-rs/issues/39)) ([ef53efd](https://github.com/JorisJonkers-dev/nv-rs/commit/ef53efd6d410142f867df000af1ace891a9e6dae))
* **linux:** package nv-viewer as a tarball with installer, launcher, desktop entry and Nix flake ([#26](https://github.com/JorisJonkers-dev/nv-rs/issues/26)) ([4c4d4ef](https://github.com/JorisJonkers-dev/nv-rs/commit/4c4d4ef79813af7203b09670048cdbe37baa3acb))
* **linux:** shader compiler seam, compile cache and the six d3dx imports off Windows ([#38](https://github.com/JorisJonkers-dev/nv-rs/issues/38)) ([febb095](https://github.com/JorisJonkers-dev/nv-rs/commit/febb0954acc4f70113e72e4bc9c556fc8d96bca9))
* **nvse-plugins:** register JohnnyGuitar NVSE and ShowOff xNVSE identities with their co-save carried, off until commands exist ([#28](https://github.com/JorisJonkers-dev/nv-rs/issues/28)) ([2762f35](https://github.com/JorisJonkers-dev/nv-rs/commit/2762f356797cd823041a83d5268475e7ece31ae6))
* **nvse:** CompileScript, the function files, the documentation dumps and the help listing (654 of 654) (slaterain/nv-rs[#562](https://github.com/JorisJonkers-dev/nv-rs/issues/562)) ([7679be6](https://github.com/JorisJonkers-dev/nv-rs/commit/7679be6a9b230170ac9e668f6c254cbea8635185))


### Bug Fixes

* **ci:** install Nix without an action the repository does not allow ([#41](https://github.com/JorisJonkers-dev/nv-rs/issues/41)) ([b6b8082](https://github.com/JorisJonkers-dev/nv-rs/commit/b6b808295e67180d2805a5c2f7fc1d488563d8d6))
* **ci:** let a job list an upstream test file before the replay brings it ([#29](https://github.com/JorisJonkers-dev/nv-rs/issues/29)) ([59c41d7](https://github.com/JorisJonkers-dev/nv-rs/commit/59c41d7df60d1568f962b15ad4c87fcee3487c70))
* **fork:** capabilities tool stands in for the whole exe image and wrap table ([#36](https://github.com/JorisJonkers-dev/nv-rs/issues/36)) ([6274d97](https://github.com/JorisJonkers-dev/nv-rs/commit/6274d9732605b5f01dd265691182cbe9acf3d887))
* **fork:** give the capabilities tool's engine the function JIP wraps at 00418b10 ([#35](https://github.com/JorisJonkers-dev/nv-rs/issues/35)) ([d46ab70](https://github.com/JorisJonkers-dev/nv-rs/commit/d46ab706577a1d2deb9e0c3d3d18c413cd44acf4))
* **fork:** make the nightly pass and move the Linux file-path code into an overlay file ([#21](https://github.com/JorisJonkers-dev/nv-rs/issues/21)) ([66c2595](https://github.com/JorisJonkers-dev/nv-rs/commit/66c2595669a12032668c333f60289ed69ba593bc))
* **fork:** map the uio crate and the jip_call_arity test into the component plan ([41048cc](https://github.com/JorisJonkers-dev/nv-rs/commit/41048cc6e929c0738167fddf1d3a54e85b194353))
* **fork:** report a blocked sync to the step summary when Issues are disabled ([#25](https://github.com/JorisJonkers-dev/nv-rs/issues/25)) ([4d88d63](https://github.com/JorisJonkers-dev/nv-rs/commit/4d88d630bcb9dc64ef10601fefa4d5d007fc3047))
* **sync:** stop a replay that would apply a commit in part ([#34](https://github.com/JorisJonkers-dev/nv-rs/issues/34)) ([d6992c7](https://github.com/JorisJonkers-dev/nv-rs/commit/d6992c774172d5b68ad3f81418d85b0b6ab573b2))

## 0.1.0 (2026-11-14)


### Bug Fixes

* **release:** start versioning at 0.1.0, not 0.1.1 ([#9](https://github.com/JorisJonkers-dev/nv-rs/issues/9)) ([974830a](https://github.com/JorisJonkers-dev/nv-rs/commit/974830aab44d03181d46ab65856dbf0397aad53a))
* **sync:** ignore the spaces before a comment in .fork/allowed-divergence ([#12](https://github.com/JorisJonkers-dev/nv-rs/issues/12)) ([b043c27](https://github.com/JorisJonkers-dev/nv-rs/commit/b043c277cd5cb89aafe58ec7dd18d09173fc3280))
* **wif:** write the federation rule's subject from the repository's immutable subject ([#15](https://github.com/JorisJonkers-dev/nv-rs/issues/15)) ([a758898](https://github.com/JorisJonkers-dev/nv-rs/commit/a7588983169decc0bb8d01dde8dfe42459c7ae78))


### Performance Improvements

* **engine-core:** copy the function table of a test engine from one built per thread ([#7](https://github.com/JorisJonkers-dev/nv-rs/issues/7)) ([52b8ab0](https://github.com/JorisJonkers-dev/nv-rs/commit/52b8ab0a79bf6bf6cc6513336a05992cdcf923bb))
* **engine-core:** run a differential case under its second control word only when the first run read or loaded it ([#8](https://github.com/JorisJonkers-dev/nv-rs/issues/8)) ([012e8d0](https://github.com/JorisJonkers-dev/nv-rs/commit/012e8d00ddc17533221bd4412893daa5ebd44e59))
