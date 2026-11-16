"""Replay of a recorded run: a self-contained local HTML page (video, frame-time graph with a cursor, hitches to
jump to, screens and scenario steps), and an mpv command with the timeline burned in as subtitles.

No external scripts or fonts: the page works offline. The video is referenced by its file name (same directory).
"""
import json
import shutil
import subprocess
from pathlib import Path

import timeline
import tools

PAGE = """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>bench replay __RUN__</title>
<style>
:root{color-scheme:light dark;--bg:#fff;--fg:#1a1a1a;--mut:#666;--line:#2a6;--bad:#d33;--grid:#ddd}
@media (prefers-color-scheme:dark){:root{--bg:#161616;--fg:#e6e6e6;--mut:#999;--line:#4c8;--bad:#f66;--grid:#333}}
body{margin:0;padding:12px 16px;background:var(--bg);color:var(--fg);font:14px system-ui,sans-serif}
video{width:100%;max-height:60vh;background:#000}
canvas{width:100%;height:180px;display:block;cursor:pointer}
.row{display:flex;gap:12px;flex-wrap:wrap;align-items:center;margin:6px 0}
button{font:inherit;padding:2px 10px}.mut{color:var(--mut)}
ul{margin:4px 0;padding-left:18px;max-height:140px;overflow:auto}li{cursor:pointer}
</style></head><body>
<h1 style="font-size:18px;margin:0 0 8px">__RUN__</h1>
__VIDEO__
<canvas id="g" width="1600" height="180"></canvas>
<div class="row"><span id="now" class="mut"></span>
<button id="prev">&larr; hitch</button><button id="next">hitch &rarr;</button>
<span class="mut">graph: frame time (ms), capped at 3x the 99th percentile; red = hitch; click to seek</span></div>
<div class="row"><b>Hitches (__NH__)</b><b>Steps (__NS__)</b><b>Screens (__NC__)</b></div>
<div class="row" style="align-items:flex-start"><ul id="lh"></ul><ul id="ls"></ul><ul id="lc"></ul></div>
<script>
const T=__DATA__;
const v=document.getElementById('video'),c=document.getElementById('g'),x=c.getContext('2d');
const dur=Math.max(T.duration_s,1),css=getComputedStyle(document.documentElement);
const col=n=>css.getPropertyValue(n).trim();
const ms=T.frames.map(f=>f[1]).sort((a,b)=>a-b),top=Math.max(8,3*(ms[Math.floor(ms.length*.99)]||16));
function draw(){const w=c.width,h=c.height;x.clearRect(0,0,w,h);
 x.strokeStyle=col('--grid');x.lineWidth=1;for(const g of [16.7,33.3,50]){if(g>top)break;const y=h-g/top*h;x.beginPath();x.moveTo(0,y);x.lineTo(w,y);x.stroke()}
 x.strokeStyle=col('--line');x.beginPath();T.frames.forEach((f,i)=>{const px=f[0]/dur*w,py=h-Math.min(f[1],top)/top*h;i?x.lineTo(px,py):x.moveTo(px,py)});x.stroke();
 x.strokeStyle=col('--bad');for(const e of T.hitches){const px=e.t/dur*w;x.beginPath();x.moveTo(px,0);x.lineTo(px,h);x.stroke()}
 x.fillStyle=col('--mut');for(const s of T.steps){x.fillRect(s.t/dur*w,h-6,1,6)}
 const cx=(v?v.currentTime:0)/dur*w;x.strokeStyle=col('--fg');x.beginPath();x.moveTo(cx,0);x.lineTo(cx,h);x.stroke();}
function at(list,t){let r=null;for(const e of list){if(e.t<=t)r=e;else break}return r}
function seek(t){if(v){v.currentTime=Math.max(0,t)}draw()}
function fill(id,list,fmt){const u=document.getElementById(id);for(const e of list){const li=document.createElement('li');li.textContent=fmt(e);li.onclick=()=>seek(Math.max(0,e.t-1));u.appendChild(li)}}
fill('lh',T.hitches,e=>e.t.toFixed(1)+' s  '+e.ms.toFixed(0)+' ms  '+e.cause);
fill('ls',T.steps,e=>e.t.toFixed(1)+' s  '+e.name);fill('lc',T.screens,e=>e.t.toFixed(1)+' s  '+e.name);
c.onclick=ev=>{const r=c.getBoundingClientRect();seek((ev.clientX-r.left)/r.width*dur)};
function jump(d){const t=v?v.currentTime:0;const l=d>0?T.hitches.filter(e=>e.t>t+1.1):T.hitches.filter(e=>e.t<t-0.1).reverse();if(l.length)seek(Math.max(0,l[0].t-1))}
document.getElementById('next').onclick=()=>jump(1);document.getElementById('prev').onclick=()=>jump(-1);
function tick(){const t=v?v.currentTime:0,s=at(T.steps,t),k=at(T.screens,t);
 document.getElementById('now').textContent=t.toFixed(1)+' s / '+dur.toFixed(1)+' s  screen: '+(k?k.name:'-')+'  step: '+(s?s.name:'-');draw();requestAnimationFrame(tick)}
tick();
</script></body></html>
"""


def page(tl):
    video = f'<video id="video" src="{tl["video"]}" controls preload="auto"></video>' if tl.get("video") else '<p class="mut">no video recorded for this run: the graph and lists only</p><span id="video"></span>'
    data = json.dumps(tl, separators=(",", ":")).replace("</", "<\\/")
    return (PAGE.replace("__RUN__", tl["run"]).replace("__VIDEO__", video).replace("__DATA__", data)
            .replace("__NH__", str(len(tl["hitches"]))).replace("__NS__", str(len(tl["steps"]))).replace("__NC__", str(len(tl["screens"]))))


def mpv_command(run_dir, video, ass):
    return ["mpv", f"--sub-file={ass}", str(Path(run_dir) / video)]


def generate(run_dir):
    """Writes timeline.json, timeline.tsv, timeline.ass and replay.html into the run directory; returns the timeline."""
    d = Path(run_dir)
    tl = timeline.build(d)
    (d / "timeline.json").write_text(json.dumps(tl))
    (d / "timeline.tsv").write_text(timeline.to_tsv(tl))
    (d / "timeline.ass").write_text(timeline.to_ass(tl))
    (d / "replay.html").write_text(page(tl))
    return tl


def burn(run_dir, tl, out_name="screen.timeline.mp4"):
    """Burns timeline.ass into a copy of the video with ffmpeg (x264); returns the path."""
    d = Path(run_dir)
    ff = tools.need("ffmpeg", "burn the timeline into the video")
    out = d / out_name
    subprocess.run([ff, "-y", "-loglevel", "error", "-i", str(d / tl["video"]), "-vf", "ass=timeline.ass", "-c:v", "libx264", "-preset", "veryfast", "-crf", "23", str(out)], cwd=d, check=True)
    return out


def cmd_replay(a, resolve):
    d = resolve(a.run)
    tl = generate(d)
    print(f"replay page: {d / 'replay.html'}  ({len(tl['hitches'])} hitches, {len(tl['steps'])} steps, {tl['duration_s']} s)")
    if not tl.get("video"):
        print("no recording in this run (bench run --record, or bench record)")
        return 0
    print("open it:   xdg-open " + str(d / "replay.html"))
    cmd = mpv_command(d, tl["video"], d / "timeline.ass")
    print("or mpv:    " + " ".join(cmd) + ("" if shutil.which("mpv") else f"   (mpv missing: {tools.HINT})"))
    if a.burn:
        print("burned:    " + str(burn(d, tl)))
    if a.play and shutil.which("mpv"):
        subprocess.run(cmd)
    return 0
