# Standalone Nix usage

`tidyfs` owns a standalone flake. It has no dependency on Dubnium, private host configuration, credentials, or external local source paths.

The committed `flake.lock` pins an independent `nixpkgs` input. Downstream compositions may deliberately make that input follow another `nixpkgs`, but that is a downstream decision rather than part of the tidyfs package contract.

## Build

From a clean checkout:

```bash
nix build
```

The named package is equivalent:

```bash
nix build .#tidyfs
```

Both resolve to `packages.${system}.tidyfs`; `packages.${system}.default` is the same derivation.

The package version, description, repository URL, and main-program name are derived from `Cargo.toml`. Rust dependencies are resolved from the committed `Cargo.lock`; the flake does not maintain a second Rust dependency manifest.

## Run

```bash
nix run .# -- --help
nix run .# -- --version
nix run .# -- scan /path/to/disposable-or-intended-root
```

`apps.${system}.default` runs the packaged `tidyfs` binary. The CLI version is also derived from the Cargo package version, so the standalone app and package metadata share the same version truth.

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

`devShells.${system}.default` provides the stable Rust/Cargo toolchain surface used by the repository plus:

- `rustfmt`
- `clippy`
- `rust-analyzer`
- `mise`
- `cargo-audit`

The existing `mise.toml`, Cargo manifests, and lockfile remain authoritative for repository tasks and Rust dependencies.

## Formatter

```bash
nix fmt
```

`formatter.${system}` uses `nixfmt-rfc-style` for Nix source formatting. Rust formatting remains part of `mise run static-analysis` / `mise run ci` through the existing `scripts/static-analysis.sh` path.

## Downstream composition

A downstream flake can consume tidyfs directly:

```nix
{
  inputs.tidyfs.url = "github:ryjen/tidyfs";

  # Optional downstream policy: deliberately share the consumer's nixpkgs.
  # inputs.tidyfs.inputs.nixpkgs.follows = "nixpkgs";
}
```

Tidyfs itself does not import Dubnium modules, paths, credentials, runner state, or host policy. Any future `dubctl tidy` integration remains a separate downstream boundary and does not widen tidyfs filesystem authority.
