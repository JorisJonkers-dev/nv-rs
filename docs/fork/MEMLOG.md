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
| `gpu_kept_bytes`, `gpu_kept_levels`, `gpu_bind_groups`, `gpu_pipelines`, `gpu_presents` | `d3d9-wgpu` counters, posted every 30 presents |
| `vram_used`, `vram_total`, `gtt_used`, `gtt_total` | the card's own memory from amdgpu sysfs (`/sys/class/drm/card*/device/mem_info_*`, the card with most VRAM); what every process holds. Appended after the other keys, and absent when the driver gives no such files |

Read-only: `Mem::heap_stats` and `d3d9_wgpu::memstats::snapshot` only read. `nv-rs-dev smoke` reports peak RSS and guest heap when the
line is present (`NV_MEMLOG=5 nv-rs-dev smoke`). A wgpu allocator report is not included: wgpu 30 would need a device handle the sampler does not hold.
