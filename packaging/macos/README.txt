nv-rs for macOS (Apple silicon and Intel)
=========================================

What this is: the nv-rs engine port's nv-viewer, built natively for macOS as one binary for Apple silicon and Intel
Macs. It checks an installation of your own Fallout: New Vegas and runs the game in a window drawn with Metal
(through wgpu). This package carries no game data, and it never copies your FalloutNV.exe.

Most people get it through Raul's Trusty Toolbox (raul), which carries this build and sets it up for an Instance.
Raul is the launcher: this archive has no launcher program and no play.sh of its own, so start the game with raul, or
with bin/nv-viewer as below.

Use
  bin/nv-viewer "<your game folder>" --check     check FalloutNV.exe is 1.4.0.525 and what a run will find (exit 0 ok, 1 not)
  bin/nv-viewer "<your game folder>"             start it

The binary is not signed or notarised yet: macOS asks before the first run (System Settings, Privacy & Security).

Files: bin/nv-viewer, share/nv-rs/ (capabilities.json, release-manifest.json and their schemas), this README.
Licence: GPL-3.0-or-later; source at https://github.com/JorisJonkers-dev/nv-rs at the release's tag.
