nv-rs for Linux (x86_64)
========================

What this is: the nv-rs engine port's nv-viewer, built natively for Linux. It checks an installation of your own
Fallout: New Vegas and runs what the port can run off Windows. It does NOT play the game yet: off Windows there is
no window, sound or graphics back end. This package carries no game data, and it never copies your FalloutNV.exe.

Install
  Unpack the tarball, then in the folder:   ./install.sh            (to ~/.local; --prefix DIR for another place)
  Or with nvmod:                            nvmod install-nv-rs
  Or with Nix:                              nix profile install github:JorisJonkers-dev/nv-rs

Use
  nv-rs-launch --set-data "<your game folder>"    remember the folder (the one with FalloutNV.exe, or its Data folder)
  nv-rs-launch --check                            check FalloutNV.exe is 1.4.0.525 and what a run will find
  nv-rs-launch "<your game folder>"               start nv-viewer on it
  nv-viewer "<your game folder>" --check          the same check without the launcher (exit 0 ok, 1 not)

The desktop entry "nv-rs" runs nv-rs-launch in a terminal with the saved folder.

Files
  bin/nv-viewer, bin/nv-rs-launch, install.sh, share/applications/nv-rs.desktop, share/icons/..., share/nv-rs/*.json
  (capabilities.json, release-manifest.json and their schemas: names, counts and hashes, no game data).
