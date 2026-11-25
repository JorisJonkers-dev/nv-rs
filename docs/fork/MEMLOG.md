# Memory timeline (`NV_MEMLOG`)

Off by default. `NV_MEMLOG=<seconds>` (`1` is one second; `0.5` is allowed) prints one line per interval on the game's main
loop (no thread; one clock read per frame when idle). Numbers only, `key=value`, bytes unless noted:

| Key | Meaning |
| --- | --- |
| `t`, `n`, `frame` | seconds since start (3 decimals), line count, main frames so far |
| `heap_used`, `heap_peak`, `heap_cap` | guest heap in use (both ranges: capacity minus what it can still hand out), the highest `heap_used` the sampler saw, capacity |
| `heap_free`, `heap_largest_free` | bytes in the bins, largest free block |
| `heap_top`, `heap_peak_addr` | next fresh guest address, highest address reached (decimal) |
| `rss`, `rss_peak` | host resident set and its peak (`VmRSS`/`VmHWM` in `/proc/self/status`; `GetProcessMemoryInfo` on Windows) |
| `vm_size`, `rss_anon` | the process's virtual size (`VmSize`, the guest's 4 GiB reservation counted in full) and the anonymous part of the resident set (`RssAnon`: committed guest pages, host heap, the GPU layer's CPU copies). Linux only; appended after the `gpu_*` keys, absent elsewhere |
| `gpu_kept_bytes`, `gpu_kept_levels`, `gpu_bind_groups`, `gpu_pipelines`, `gpu_presents` | `d3d9-wgpu` counters, posted every 30 presents |
| `vram_used`, `vram_total`, `gtt_used`, `gtt_total` | the card's own memory from amdgpu sysfs (`/sys/class/drm/card*/device/mem_info_*`, the card with most VRAM); what every process holds. Appended after the other keys, and absent when the driver gives no such files |
| `gpu_tex_bc`, `gpu_tex_plain`, `gpu_targets`, `gpu_targets_msaa`, `gpu_depth`, `gpu_vertex`, `gpu_index`, `gpu_own` | video memory the layer made, by kind (its budget's charges): sampled textures kept block-compressed, other sampled textures, colour targets of one sample (with their copies), multisampled targets, depth surfaces, the game's vertex and index buffers, the layer's own charged buffers. Appended after the card keys |
| `gpu_textures`, `gpu_buffer_copies`, `gpu_modules` | live textures; the host copy kept of every game vertex and index buffer (bytes); compiled shader modules |
| `gpu_alloc_allocated`, `gpu_alloc_reserved` | the driver allocator's report: bytes handed out and bytes held in its blocks (read every 300 presents; 0 where the backend gives none). The difference is held memory no object uses |
| `gpu_reads`, `gpu_read_bytes` | `ReadSurface` answers so far and their bytes |
| `proc_vram`, `proc_gtt` | this process's own memory on the card (`drm-memory-vram`, `drm-memory-gtt` in `/proc/self/fdinfo`, each DRM client once; Linux). Unlike `vram_used` it holds no other process's. Last on the line |

Read-only: `Mem::heap_stats` and `d3d9_wgpu::memstats::snapshot` only read. `nv-rs-dev smoke` reports peak RSS and guest heap when the
line is present (`NV_MEMLOG=5 nv-rs-dev smoke`). Every 1800 presents the log also gets two info lines from `d3d9`: the live textures by D3D format (count, MB, single-level ones, ones 1024 or more on a side) and the allocator's blocks (sizes, empty and under a quarter used, the largest users by name).

The driver allocator's block sizes: `NV_GPU_MEMORY_HINT=performance` is wgpu's 128 to 256 MB blocks, `memory` its 8 to 64 MB ones, `MIN-MAX` (MB, 4 to 256) blocks of those sizes; the default is 4 to 16 MB (`memory_hints` in d3d9-wgpu's device). On an 18-minute soak of twelve places with a 142-plugin load order the blocks held were 4.7 GB with the large blocks, 4.2 GB with 8 to 64 MB and 2.6 GB with 4 to 16 MB, with the same textures live. `NV_GPU_ALLOC_WATCH=1` says the allocator's blocks each time they grow by 128 MB past the most seen (one report a present, for finding what fills them).
