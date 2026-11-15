{
  description = "nv-rs: Fallout: New Vegas engine port (fork), native Linux nv-viewer";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      # The one compiler every build of the fork uses (rust-toolchain.toml).
      toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      rustPlatform = pkgs.makeRustPlatform {
        cargo = toolchain;
        rustc = toolchain;
      };
      version = (builtins.fromTOML (builtins.readFile ./viewer/Cargo.toml)).package.version;

      nv-viewer = rustPlatform.buildRustPackage {
        pname = "nv-rs";
        inherit version;
        src = pkgs.lib.cleanSource ./.;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "nv-viewer" ];
        nativeBuildInputs = [ pkgs.makeWrapper ];
        # The test suite runs in the fork's CI; this build is the package. The smoke test below runs the result.
        doCheck = false;
        postInstall = ''
          # The shader compiler loads libvkd3d-shader, and the sound output libpulse-simple, at run time (docs/fork/LINUX.md): the package carries both.
          wrapProgram $out/bin/nv-viewer --set-default NV_VKD3D_SHADER ${pkgs.vkd3d.lib}/lib/libvkd3d-shader.so.1 --set-default NV_PULSE_SIMPLE ${pkgs.libpulseaudio}/lib/libpulse-simple.so.0
          install -Dm755 packaging/linux/nv-rs-launch $out/bin/nv-rs-launch
          install -Dm644 packaging/linux/nv-rs.png $out/share/icons/hicolor/256x256/apps/nv-rs.png
          install -Dm644 packaging/linux/README.txt $out/share/doc/nv-rs/README.txt
          mkdir -p $out/share/applications
          sed "s|^Exec=nv-rs-launch|Exec=$out/bin/nv-rs-launch|" packaging/linux/nv-rs.desktop > $out/share/applications/nv-rs.desktop
        '';
        meta = {
          description = "Fallout: New Vegas engine port: nv-viewer checks and runs an installation of the player's own game (no game data included)";
          homepage = "https://github.com/JorisJonkers-dev/nv-rs";
          mainProgram = "nv-viewer";
          platforms = [ system ];
        };
      };
    in
    {
      packages.${system} = {
        inherit nv-viewer;
        default = nv-viewer;
      };

      apps.${system}.default = {
        type = "app";
        program = "${nv-viewer}/bin/nv-rs-launch";
      };

      # `nix flake check` builds the package and runs nv-viewer --check on it (no game data needed).
      checks.${system}.smoke = pkgs.runCommand "nv-rs-smoke" { nativeBuildInputs = [ pkgs.gnugrep pkgs.coreutils ]; } ''
        sh ${./packaging/linux/smoke-test.sh} ${nv-viewer}
        touch $out
      '';

      devShells.${system} = {
      default = pkgs.mkShell {
        packages = [ toolchain pkgs.pkg-config ];
      };

      # The environment the fork's Linux CI tests run in (docs/fork/CI-NIX.md): the pinned compiler, a recent
      # libvkd3d-shader (the runners' apt one is too old for HLSL to d3dbc), the software Vulkan driver (lavapipe)
      # with the Vulkan loader, and a virtual X server. Everything, the C library included, comes from the one pinned
      # nixpkgs, so the libraries `dlopen`ed by a test binary match its C library. `nix develop .#ci --command ...`.
      ci = pkgs.mkShell {
        packages = [ toolchain pkgs.pkg-config pkgs.xvfb pkgs.xvfb-run pkgs.vulkan-tools ];
        shellHook = ''
          export NV_VKD3D_SHADER=${pkgs.vkd3d.lib}/lib/libvkd3d-shader.so.1
          # The vkd3d tests fail, not skip, when the library cannot be loaded (d3dx-shader's NV_REQUIRE_VKD3D).
          export NV_REQUIRE_VKD3D=1
          # The headless Direct3D 9 frame test (engine-platform, d3d9real/fork_d3d9rec.rs) fails, not skips, without an adapter.
          export NV_REQUIRE_GPU=1
          export VK_ICD_FILENAMES=${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.x86_64.json
          export LD_LIBRARY_PATH=${pkgs.vulkan-loader}/lib''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
          export NV_WGPU_BACKEND=software
          export RUST_TEST_NOCAPTURE=1
        '';
      };
      };
    };
}
