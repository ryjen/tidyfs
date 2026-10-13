# Canonical Nix build and release qualification

`tidyfs` owns a standalone canonical flake. It has no dependency on Dubnium, private host configuration, credentials, or external local source paths.

The root flake is the source repository's build, test, and release-qualification contract. A separate `distribution/` subflake in this same public repository is the binary-oriented downstream installation surface; it fetches immutable release bytes rather than rebuilding the source tree.

The committed `flake.lock` pins an independent `nixpkgs` input. Canonical build/release work may deliberately compose it with another pinned `nixpkgs`, but that does not transfer product-version authority to a downstream repository.

## Build

From a canonical checkout:

```bash
nix build
```

The named package is equivalent:

```bash
nix build .#tidyfs
```

Both resolve to `packages.${system}.tidyfs`; `packages.${system}.default` is the same derivation.

The package version, description, repository URL, and main-program name are derived from `Cargo.toml`. Rust dependencies are resolved from the committed `Cargo.lock`; the canonical flake does not maintain a second Rust dependency manifest.

The package installs both the executable and its section-1 manual page:

```text
bin/tidyfs
share/man/man1/tidyfs.1
```

The package check verifies the executable contract and that `man tidyfs` resolves from the packaged manual path.

## Run

```bash
nix run .# -- --help
nix run .# -- --version
nix run .# -- scan /path/to/disposable-or-intended-root
```

`apps.${system}.default` runs the canonically built `tidyfs` binary. The CLI version is derived from the Cargo package version, so source, package metadata, CLI output, and release-tag validation share one product identity.

Nix packaging does not perform cleanup or other filesystem maintenance automatically. A normal `nix build`, `nix flake check`, or development-shell entry only builds/tests the project. Real tidyfs mutation still requires the CLI's existing explicit safety and interactive gates.

## Check

```bash
nix flake check
```

`checks.${system}.default` is the package derivation. Its check phase runs the repository's canonical deterministic quality task:

```bash
mise run ci
```

That task covers shared formatting/Clippy static analysis, the full deterministic Rust test suite, and Cargo package verification. Existing filesystem-effect tests operate only on disposable temporary fixtures; the Nix check does not target a developer's real filesystem.

Coverage-guided fuzz campaigns remain outside `checks.default` because they are scheduled/manual and non-deterministic by design. PR CI continues to compile the maintained fuzz harness separately.

## Development shell

```bash
nix develop
```

`devShells.${system}.default` provides the stable Rust/Cargo toolchain surface used by the repository plus package/CI tooling, including:

- `rustfmt`
- `clippy`
- `rust-analyzer`
- `mise`
- `cargo-audit`
- `gh`
- `man-db`

The existing `mise.toml`, Cargo manifests, and lockfile remain authoritative for canonical repository tasks and Rust dependencies.

On Linux, `devShells.x86_64-linux.release` is the dedicated distributable-release environment. It keeps the normal host compiler for build scripts and uses the musl compiler only for the explicit `x86_64-unknown-linux-musl` target. It also owns the release-time `gh`, `man`, archive, checksum, and ELF-inspection tools rather than inheriting them from the JIT runner image.

## Linux release artifact

The canonical Linux release is a static `x86_64-unknown-linux-musl` archive. Release qualification rejects a dynamic ELF interpreter, shared-library `NEEDED` entries, and runtime `RPATH`/`RUNPATH` entries, then executes the packaged binary outside the Nix development shell with a minimal environment.

The archive layout is:

```text
tidyfs-X.Y.Z-x86_64-unknown-linux-musl/
  tidyfs -> bin/tidyfs
  bin/tidyfs
  share/man/man1/tidyfs.1
  README.md
  LICENSE-MIT
  LICENSE-APACHE
```

The top-level `tidyfs` entry is a compatibility symlink; `bin/tidyfs` is the canonical executable path. Packaging verifies the archive allowlist, checksum, extracted executable mode, symlink target, CLI version, and manual lookup from the extracted artifact.

Every `main` commit receives release-bundle qualification because the deterministic archive metadata is tied to the exact release commit. Creating an immutable release tag therefore requires successful `main`-push CI and Release runs for the exact current `main` SHA. Tag/manual publication revalidates that identity before publishing.

The `.sha256` file is integrity evidence for the release archive. This contract does **not** claim that SLSA provenance or artifact signing is already implemented; those are separate trust-hardening additions if required.

## Formatter

```bash
nix fmt
```

`formatter.${system}` uses `nixfmt-rfc-style` for Nix source formatting. Rust formatting remains part of `mise run static-analysis` / `mise run ci` through the existing `scripts/static-analysis.sh` path.

## Public binary distribution

The public installation path is tracked by #82:

```text
public source + reviewed vX.Y.Z commit/tag
  -> deterministic immutable release archive + checksum evidence
  -> distribution/ binary flake with pinned artifact hash
  -> downstream committed flake.lock
```

The `distribution/` flake fetches the immutable authorized release asset from `ryjen/tidyfs`; it does not compile the current source tree. It installs the same executable and section-1 manual and verifies the release bytes with the pinned Nix hash.

The canonical release commit is the source/release execution identity and the immutable `vX.Y.Z` tag is its human-readable label. The distribution metadata binds to that release identity and digest; it is not a second product-version authority.

High-assurance consumers such as Dubnium should pin an exact reviewed `ryjen/tidyfs` commit with `?dir=distribution` and commit the resulting lock state. If dotfiles also declares the tidyfs distribution input, the composed host should make that input follow its single top-level tidyfs distribution pin.

## Repository visibility

`ryjen/tidyfs` remains public. Source privacy is not part of the authorization model, the implementation has already been publicly disclosed, and the runtime/security design assumes attacker knowledge of the code. Keeping a separate binary-only repository therefore adds coordination cost without a meaningful security boundary.

Tidyfs itself does not import Dubnium modules, paths, credentials, runner state, or host policy. Any future `dubctl tidy` integration remains a separate downstream boundary and does not widen tidyfs filesystem authority.
