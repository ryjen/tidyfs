# Binary distribution

This directory is the binary-oriented public distribution surface for `tidyfs`. It replaces the separate `ryjen/tidyfs-community` repository.

The source, build, test, release, documentation, and binary-distribution contracts now live in one public repository. The distribution flake still consumes an immutable release archive by cryptographic hash; it does **not** rebuild the current source tree.

## Current release

- version: `0.7.0`
- canonical tag: `v0.7.0`
- canonical commit: `4ce8406a903ded92fcc3126933ac64aab700d626`
- target: `x86_64-unknown-linux-musl`
- archive: `tidyfs-0.7.0-x86_64-unknown-linux-musl.tar.gz`
- SHA-256: `fe818f3e609185e8af9dc9fd21f41e132fd42907168255c146c21dd73cd60bf9`

`release.json` is the machine-readable distribution identity. The flake fetches the immutable GitHub release asset from this repository and verifies it using the pinned Nix hash.

## Consumption

Pin an exact reviewed `tidyfs` commit and select this subflake:

```nix
tidyfs.url = "github:ryjen/tidyfs/<commit>?dir=distribution";
```

Downstream consumers should commit their `flake.lock`. High-assurance consumers may continue to validate the reported version, manual page, and synthetic read-only behavior independently.

## Trust model

- the immutable release tag/asset remains the product-version authority;
- the distribution flake is packaging metadata for those release bytes, not a second version authority;
- SHA-256/Nix hashing proves byte identity and integrity, not reproducible-build or SLSA provenance;
- distributed binaries are assumed reverse engineerable and contain no secrets or hidden authorization material;
- source visibility is not an authorization control;
- rollback means pinning an earlier reviewed commit/release, never mutating an existing release identity.

## Historical repository

`ryjen/tidyfs-community` existed to separate a public binary mirror from a potentially private canonical source repository. Since `ryjen/tidyfs` remains public and its security model assumes full attacker knowledge, that split no longer justifies a second repository.
