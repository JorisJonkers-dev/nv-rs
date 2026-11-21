nv-rs for Linux (x86_64)
========================

What this is: the nv-rs engine port's nv-viewer, built natively for Linux. It checks an installation of your own
Fallout: New Vegas and runs the game in a window (graphics through wgpu, Vulkan on Linux; sound through the system's
sound server). The Linux build is young: expect rough edges and report each stop with its log. This package carries no
game data, and it never copies your FalloutNV.exe.

Most people get it through Raul's Trusty Toolbox (raul), which carries this build and starts it for an Instance.

Install
  Unpack the tarball, then in the folder:   ./install.sh            (to ~/.local; --prefix DIR for another place)
  Or with nvmod:                            nvmod install-nv-rs
  Or with Nix:                              nix profile install github:JorisJonkers-dev/nv-rs

Use
  nv-rs-launch --set-data "<your game folder>"    remember the folder (the one with FalloutNV.exe, or its Data folder)
  nv-rs-launch --check                            check FalloutNV.exe is 1.4.0.525 and what a run will find
  nv-rs-launch "<your game folder>"               start nv-viewer on it (the launcher sets NV_D3D9=wgpu unless you set it)
  A bare nv-viewer needs NV_D3D9=wgpu in its environment to open a window.
  nv-viewer "<your game folder>" --check          the same check without the launcher (exit 0 ok, 1 not)

The desktop entry "nv-rs" runs nv-rs-launch in a terminal with the saved folder.

Files
  bin/nv-viewer, bin/nv-rs-launch, install.sh, share/applications/nv-rs.desktop, share/icons/..., share/nv-rs/*.json
  (capabilities.json, release-manifest.json and their schemas: names, counts and hashes, no game data).
