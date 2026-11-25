# Shader overrides

A shader override draws one of the game's pixel shaders with a WGSL program of the fork's own instead of the
conversion of the game's bytecode, and lets that program read the scene's depth. It is how the fork changes what a
game shader computes without touching the translated engine: the engine still sets the same pass, constants and
textures, and only the program that runs on the GPU is ours. Nothing is replaced unless a setting asks for it (or the
engineers' folder below); without one the device is exactly what it was.

The code is `crates/d3d9-wgpu/src/shader_override.rs` (the device's part and the program's interface),
`crates/d3d9-wgpu/src/sm/wgsl.rs` (`convert_replaced`, the module around the program) and
`crates/host/src/shader_overrides.rs` (what the run asks for).

## What is replaced

A shader is named as the game's shader packages name it (`Data/Shaders/shaderpackageNNN.sdp`), for example
`WATER033.pso`. At the start the host reads the packages of the data folder (the winning package of each name, as
the game finds them), takes the bytecode of every shader of that name (one name can have a different bytecode in
each package) and gives the device their hashes. Only the hashes are kept; no game bytecode is copied anywhere.

## The program

The program is WGSL that defines `fn nv_main()`. The device builds the module around it from the game shader's
own signature: the same bindings, the same inputs and outputs, the same steps after the body (the alpha test, the
occlusion count). None of the game shader's instructions are converted. The program reads:

| Name | What it is |
| --- | --- |
| `nv_texcoord[n]`, `nv_color[n]` | the shader's inputs by meaning (`TEXCOORDn`, `COLORn`), whatever registers its profile put them in |
| `nv_c(n)` | the float constant `c<n>` as the game set it |
| `nv_const_<Name>()` | the first register of each float constant the shader's constant table names (`nv_const_EyePos()`) |
| `nv_sample_<Name>(uv)` | each 2D sampler the table names, sampled as the shader's own `texld` would be |
| `nv_frag` | the pixel's position (its centre in pixels, its depth from 0 near to 1 far) |
| `nv_scene_depth()` | the distance along the view axis of what the scene's depth held at this pixel before the frame's first overridden draw (the far distance where nothing was drawn: the sky) |
| `nv_own_depth()` | the pixel's own distance along the view axis |
| `NV_VIEW` | the frame's near and far planes and the depth surface's size |

It writes the outputs: `oC0` (and `oC1`.. where the shader has them).

The program's first lines that start with `// nv:` say how it is drawn, by words:

| Word | What it does |
| --- | --- |
| `late` | its draws are kept and drawn where the world is drawn (after the world pass, at the frame's camera words), so the scene's depth it reads holds the whole world, also what the game draws after the shader's own draws; they keep their order, states and targets |
| `blend` | its draws blend over what is behind them by the program's alpha (source alpha over one minus source alpha), whatever blending the game set |
| `registers=N` | it reads the float constants `c0` to `c<N-1>` (all of them when not said) |
| `patch` | the game's shader is converted as without the override and the program only patches the float constants it reads (below) |

## Patches

A program whose `// nv:` lines say `patch` does not replace the game's shader: the shader is converted as without an
override, and every float constant its instructions read from the device (not the shader's own `def` values; a read
through an address register too, by the register it reaches) is the answer of the program's `fn nv_patch_c(n: i32, v: vec4<f32>) -> vec4<f32>` for the register `n` and the device's value
`v`. The program reads the same names as a replacement (`nv_frag`, `nv_texcoord[n]`, `nv_const_<Name>()`, ...) and
`nv_reg_<Name>`, each float constant's first register by the shader's table. `water.near_fade` is one
(docs/fork/GRAPHICS.md, water); `d3d9_wgpu::shader_override::names_naming` picks the shaders a patch fits by the
constants they name.

## How it is drawn

An overridden draw takes a pipeline of its own, made on a thread of its own the first time it is needed. Until it is
made, and wherever the scene's depth cannot be read (no camera for the frame yet, a depth surface that cannot be
sampled or that does not cover the target), the draw takes the game's shader, as without the override.

The scene's depth reaches the program through bind group 3. At the frame's first overridden draw on a depth
surface, the device ends the render pass, turns the depth's first sample into distances in a texture of its own
(through the frame's near and far planes, the camera words recorded where the world is drawn), and goes on. Later
overridden draws of the frame on that surface read the same distances, so a draw never reads what an overridden
draw before it wrote. The camera words are recorded for the new lighting; with the new lighting off, the platform
records them for the overrides alone (`engine_platform::d3d9real::want_frame_view`).

A late draw keeps what the pass is given for it (its pipeline, textures, vertices, the pass's settings) and the bytes
of its registers, which are written again when it is drawn: the batch may have been sent by then. A late draw of a
frame that never reaches the world's point (no camera words), or whose targets are not the bound ones there, is
dropped and counted, never drawn into a later frame. A draw from the game's memory (`DrawPrimitiveUP`) is never
late.

An overridden draw is never held back to be drawn with others, and never written into the pipeline list for the
next start: the list stays what the game's own draws make. Cost: one full-screen step a frame while an overridden
shader draws, and one pipeline per state the overridden draws use.

## The engineers' folder

`NV_SHADER_OVERRIDES=<folder>` replaces each shader that a `<shader name>.wgsl` file in the folder names (for example
`WATER033.pso.wgsl`), with every float constant given to it. A folder's program wins over a setting's for the same
shader. It is for trying a program without a build; the log says which shaders it found:

```
nv-rs: shader override WATER033.pso (NV_SHADER_OVERRIDES): 1 bytecode(s) in 16 package(s) drawn with the fork's own shader
NV_D3D9 wgpu: shader override WATER033.pso (NV_SHADER_OVERRIDES): first drawn at present 12 on a 2560x1440 depth surface (1 samples), near 5, far 353840
```

A program WGSL refuses is said on the log (`its pipeline: ...`) and the game's shader draws.
