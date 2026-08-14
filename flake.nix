{
  description = "tidyfs — conservative filesystem cleanup with deterministic policy";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
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
            pname = "tidyfs";
            version = "0.6.1";
            src = self;

            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [
              pkgs.clippy
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
              "$out/bin/tidyfs" --help >/dev/null
              "$out/bin/tidyfs" --version >/dev/null
            '';

            meta = {
              description = "Conservative disk usage scanner and cleanup planner for developer machines";
              homepage = "https://github.com/ryjen/tidyfs";
              license = with pkgs.lib.licenses; [
                mit
                asl20
              ];
              mainProgram = "tidyfs";
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
              pkgs.cargo
              pkgs.cargo-audit
              pkgs.clippy
              pkgs.mise
              pkgs.rust-analyzer
              pkgs.rustc
              pkgs.rustfmt
            ];
          };
        }
      );

      formatter = forAllSystems (system: (import nixpkgs { inherit system; }).nixfmt-rfc-style);
    };
}
