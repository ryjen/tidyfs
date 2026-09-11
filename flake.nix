{
  description = "tidyfs — conservative filesystem cleanup with deterministic policy";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          tidyfs = pkgs.rustPlatform.buildRustPackage {
            pname = cargoToml.package.name;
            version = cargoToml.package.version;
            src = self;

            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [
              pkgs.clippy
              pkgs.man-db
              pkgs.mise
              pkgs.rustfmt
            ];

            # Keep Nix on the same deterministic quality surface used by local/CI builds.
            # buildRustPackage supplies Cargo's vendored/offline dependency environment.
            checkPhase = ''
              runHook preCheck
              export HOME="$TMPDIR/home"
              export MISE_DATA_DIR="$TMPDIR/mise-data"
              export MISE_CACHE_DIR="$TMPDIR/mise-cache"
              mkdir -p "$HOME" "$MISE_DATA_DIR" "$MISE_CACHE_DIR"
              mise trust
              mise run ci
              runHook postCheck
            '';

            postInstall = ''
              install -Dm444 man/tidyfs.1 "$out/share/man/man1/tidyfs.1"
              "$out/bin/tidyfs" --help >/dev/null
              test "$("$out/bin/tidyfs" --version)" = "tidyfs ${cargoToml.package.version}"
              MANPATH="$out/share/man" man -w tidyfs >/dev/null
            '';

            meta = {
              description = cargoToml.package.description;
              homepage = cargoToml.package.repository;
              license = with pkgs.lib.licenses; [
                mit
                asl20
              ];
              mainProgram = cargoToml.package.name;
              platforms = pkgs.lib.platforms.unix;
            };
          };
        in
        {
          inherit tidyfs;
          default = tidyfs;
        }
      );

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.tidyfs}/bin/tidyfs";
          meta.description = "Run tidyfs";
        };
      });

      # The package derivation itself runs `mise run ci` in checkPhase, so the default
      # flake check covers the standalone build and the repository's deterministic gate.
      checks = forAllSystems (system: {
        default = self.packages.${system}.tidyfs;
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.bash
              pkgs.cargo
              pkgs.cargo-audit
              pkgs.clippy
              pkgs.coreutils
              pkgs.diffutils
              pkgs.gh
              pkgs.git
              pkgs.gnused
              pkgs.gnutar
              pkgs.gzip
              pkgs.man-db
              pkgs.mise
              pkgs.python3
              pkgs.rust-analyzer
              pkgs.rustc
              pkgs.rustfmt
              pkgs.stdenv.cc
            ];
          };
        }
        // pkgs.lib.optionalAttrs (system == "x86_64-linux") {
          # Keep the normal host toolchain for proc-macros/build scripts. Only the
          # explicit x86_64 musl target uses the musl C compiler/linker below. Release
          # qualification also owns its man/GitHub CLI dependencies here rather than
          # inheriting mutable runner-image tools.
          release = pkgs.mkShell {
            packages = [
              pkgs.bash
              pkgs.binutils
              pkgs.coreutils
              pkgs.diffutils
              pkgs.gh
              pkgs.git
              pkgs.gnused
              pkgs.gnutar
              pkgs.gzip
              pkgs.man-db
              pkgs.python3
              pkgs.stdenv.cc
            ];
            CC_x86_64_unknown_linux_musl = "${pkgs.pkgsMusl.stdenv.cc}/bin/cc";
            CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = "${pkgs.pkgsMusl.stdenv.cc}/bin/cc";
          };
        }
      );

      formatter = forAllSystems (system: (import nixpkgs { inherit system; }).nixfmt-rfc-style);
    };
}
