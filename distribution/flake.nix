{
  description = "Binary distribution for tidyfs";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      release = builtins.fromJSON (builtins.readFile ./release.json);
      supportedSystems = [ "x86_64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      expectedPrefix = "https://github.com/ryjen/tidyfs/releases/download/";
      sourceIsCanonicalRelease =
        builtins.substring 0 (builtins.stringLength expectedPrefix) release.source_url == expectedPrefix;
    in
    assert release.version == "0.7.0";
    assert release.canonical_tag == "v0.7.0";
    assert release.canonical_commit == "4ce8406a903ded92fcc3126933ac64aab700d626";
    assert release.target == "x86_64-unknown-linux-musl";
    assert sourceIsCanonicalRelease;
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          archive = pkgs.fetchurl {
            url = release.source_url;
            hash = release.nix_hash;
          };
          bundleName = "tidyfs-${release.version}-${release.target}";
          tidyfs = pkgs.stdenvNoCC.mkDerivation {
            pname = "tidyfs";
            version = release.version;
            src = archive;
            dontUnpack = true;

            nativeBuildInputs = [ pkgs.gnutar pkgs.gzip pkgs.man-db ];

            installPhase = ''
              runHook preInstall
              set -euo pipefail
              root="$TMPDIR/tidyfs-distribution-unpack"
              mkdir -p "$root"
              tar -xzf "$src" -C "$root"
              bundle="$root/${bundleName}"

              test -x "$bundle/bin/tidyfs"
              test -r "$bundle/share/man/man1/tidyfs.1"
              test "$(stat -c '%a' "$bundle/bin/tidyfs")" = 755
              test "$(stat -c '%a' "$bundle/share/man/man1/tidyfs.1")" = 644

              install -Dm755 "$bundle/bin/tidyfs" "$out/bin/tidyfs"
              install -Dm644 "$bundle/share/man/man1/tidyfs.1" "$out/share/man/man1/tidyfs.1"
              runHook postInstall
            '';

            doInstallCheck = true;
            installCheckPhase = ''
              set -euo pipefail
              test "$("$out/bin/tidyfs" --version)" = "tidyfs ${release.version}"
              "$out/bin/tidyfs" --help >/dev/null
              MANPATH="$out/share/man" man -w tidyfs >/dev/null
            '';

            meta = {
              description = "Conservative filesystem cleanup with deterministic policy";
              homepage = "https://github.com/ryjen/tidyfs";
              license = with pkgs.lib.licenses; [ mit asl20 ];
              mainProgram = "tidyfs";
              platforms = [ "x86_64-linux" ];
              sourceProvenance = with pkgs.lib.sourceTypes; [ binaryNativeCode ];
            };
          };
        in {
          inherit tidyfs;
          default = tidyfs;
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.tidyfs}/bin/tidyfs";
          meta.description = "Run tidyfs";
        };
      });

      checks = forAllSystems (system: {
        default = self.packages.${system}.tidyfs;
      });
    };
}
