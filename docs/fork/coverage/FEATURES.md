# Coverage catalogue

Generated on fork by tally from `.fork/catalogue` (docs/fork/CATALOGUE.md). Do not edit.

| | Built | Has a test | Tested on some OSes | Tested on all OSes | Not built yet | Needs the game to test | Not needed | Works via the game's code |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Total, except Engine functions** | 1981/3020 | 1111 | 1111 | 1100 | 1039 | 0 | 0 | 172 |
| **Direct3D 9 on wgpu** | 123/222 | 0 | 0 | 0 | 99 | 0 | 0 | 0 |
| IDirect3DDevice9 methods on wgpu (`d3d9.methods`) | 80/119 | 0 | 0 | 0 | 39 | 0 | 0 | 0 |
| Direct3D 9 render states on wgpu (`d3d9.states`) | 43/103 | 0 | 0 | 0 | 60 | 0 | 0 | 0 |
| **Engine functions** | 69459/69459 | 27 | 27 | 26 | 0 | 0 | 29 | 0 |
| FalloutNV.exe functions (`engine.functions`) | 69459/69459 | 27 | 27 | 26 | 0 | 0 | 29 | 0 |
| **Script extender commands** | 1835/2470 | 1098 | 1098 | 1095 | 635 | 0 | 0 | 0 |
| AnhNVSE commands (`extenders.anh_nvse`) | 2/2 | 2 | 2 | 0 | 0 | 0 | 0 | 0 |
| JIP LN NVSE commands (`extenders.jip_ln`) | 1117/1117 | 1034 | 1034 | 1034 | 0 | 0 | 0 | 0 |
| JohnnyGuitar NVSE commands (`extenders.johnnyguitar`) | 23/412 | 23 | 23 | 23 | 389 | 0 | 0 | 0 |
| kNVSE commands (`extenders.knvse`) | 1/59 | 1 | 1 | 0 | 58 | 0 | 0 | 0 |
| ShowOff xNVSE commands (`extenders.showoff`) | 35/215 | 35 | 35 | 35 | 180 | 0 | 0 | 0 |
| lStewieAl's Tweaks commands (`extenders.stewie_tweaks`) | 0/8 | 0 | 0 | 0 | 8 | 0 | 0 | 0 |
| Tale of Two Wastelands plugin (ttw_nvse) commands (`extenders.ttw_nvse`) | 3/3 | 3 | 3 | 3 | 0 | 0 | 0 | 0 |
| xNVSE commands (`extenders.xnvse`) | 654/654 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| **Engine features** | 18/236 | 13 | 13 | 5 | 218 | 0 | 0 | 172 |
| Actors and animation (`features.actors`) | 0/19 | 0 | 0 | 0 | 19 | 0 | 0 | 19 |
| Audio and dialogue (`features.audio`) | 0/6 | 0 | 0 | 0 | 6 | 0 | 0 | 6 |
| Rendering backend (wgpu and Vulkan) (`features.backend`) | 8/18 | 5 | 5 | 5 | 10 | 0 | 0 | 0 |
| 64-bit readiness (`features.bit64`) | 2/11 | 0 | 0 | 0 | 9 | 0 | 0 | 0 |
| Combat (`features.combat`) | 0/8 | 0 | 0 | 0 | 8 | 0 | 0 | 8 |
| Data, plugins and saves (`features.data`) | 0/7 | 0 | 0 | 0 | 7 | 0 | 0 | 7 |
| Direct3D 9 roll-ups (`features.direct3d`) | 0/2 | 0 | 0 | 0 | 2 | 0 | 0 | 0 |
| Gameplay systems (`features.gameplay`) | 0/15 | 0 | 0 | 0 | 15 | 0 | 0 | 15 |
| Navigation and pathfinding (`features.navigation`) | 0/9 | 0 | 0 | 0 | 9 | 0 | 0 | 9 |
| Physics and collision (`features.physics`) | 0/6 | 0 | 0 | 0 | 6 | 0 | 0 | 6 |
| Platform services (`features.platform`) | 0/10 | 0 | 0 | 0 | 10 | 0 | 0 | 10 |
| Plugin DLLs of mods (plugin host) (`features.plugins`) | 8/19 | 8 | 8 | 0 | 11 | 0 | 0 | 0 |
| Rendering (`features.rendering`) | 0/40 | 0 | 0 | 0 | 40 | 0 | 0 | 40 |
| Scripting (`features.scripting`) | 0/6 | 0 | 0 | 0 | 6 | 0 | 0 | 6 |
| Multi-threading (`features.threading`) | 0/14 | 0 | 0 | 0 | 14 | 0 | 0 | 0 |
| Menus and HUD (`features.ui`) | 0/25 | 0 | 0 | 0 | 25 | 0 | 0 | 25 |
| World (`features.world`) | 0/21 | 0 | 0 | 0 | 21 | 0 | 0 | 21 |
| **Graphics features** | 5/92 | 0 | 0 | 0 | 87 | 0 | 0 | 0 |
| Graphics plan capabilities (`gfx.features`) | 5/92 | 0 | 0 | 0 | 87 | 0 | 0 | 0 |

| Average share | Native | Works via the game's code | Features |
| --- | ---: | ---: | ---: |
| **Engine features** | 2.3% | 97.7% | 172 |
| Actors and animation (`features.actors`) | 1.7% | 98.3% | 19 |
| Audio and dialogue (`features.audio`) | 0.8% | 99.2% | 6 |
| Combat (`features.combat`) | 0.1% | 99.9% | 8 |
| Data, plugins and saves (`features.data`) | 1.1% | 98.9% | 7 |
| Gameplay systems (`features.gameplay`) | 0.3% | 99.7% | 15 |
| Navigation and pathfinding (`features.navigation`) | 5.6% | 94.4% | 9 |
| Physics and collision (`features.physics`) | 0.4% | 99.6% | 6 |
| Platform services (`features.platform`) | 0.7% | 99.3% | 10 |
| Rendering (`features.rendering`) | 5.8% | 94.2% | 40 |
| Scripting (`features.scripting`) | 3.4% | 96.6% | 6 |
| Menus and HUD (`features.ui`) | 0.7% | 99.3% | 25 |
| World (`features.world`) | 1.1% | 98.9% | 21 |
