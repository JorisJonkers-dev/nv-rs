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
        # The test suite runs in the fork's CI; this build is the package. The smoke test below runs the result.
        doCheck = false;
        postInstall = ''
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

      devShells.${system}.default = pkgs.mkShell {
        packages = [ toolchain pkgs.pkg-config ];
      };
    };
}
