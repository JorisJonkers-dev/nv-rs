# Changelog

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
