# mosh-client-rs

An independent, unofficial, wire-compatible Mosh client implementation in Rust.

## Status

This project is in the protocol-definition stage. It does not yet provide a
working Mosh session or a stable public API. The crate remains unpublished until
the interoperability and security gates in [the roadmap](docs/roadmap.md) pass.

## Goal

Build a memory-safe Mosh client core that interoperates with the stock
`mosh-server` and can support ARM64 HarmonyOS applications such as LeanTTY.
The library owns Mosh protocol state and UDP recovery. Its caller owns SSH
authentication, host verification, server startup, and terminal presentation.

## Initial contract

- Client only; no `mosh-server` implementation.
- Rust library first; no general-purpose CLI in the initial scope.
- Initial compatibility target: stock `mosh-server` 1.4.0.
- Initial network scope: IPv4 and a fixed UDP port.
- One crate until real ownership or build boundaries justify a split.
- No plugin framework or generic transport abstraction.
- No server installation, file transfer, port forwarding, or session manager.

See [architecture](docs/architecture.md), [compatibility](docs/compatibility.md),
and [project decisions](docs/decisions/0001-project-scope-and-licensing.md).

## Independent implementation

This project does not copy GPL-covered Mosh source code, comments, file
organization, or tests. Protocol behavior must come from public specifications,
papers, standards, documented black-box observations, or original analysis.
Every nontrivial compatibility rule must record its source in
[the provenance log](docs/provenance.md).

The project uses established, permissively licensed cryptographic primitives.
It does not implement cryptographic algorithms from scratch.

## Development

Run Rust work in the default WSL distribution from
`/mnt/c/repos/mosh-client-rs`:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## License

Licensed under either the [Apache License, Version 2.0](LICENSE-APACHE) or the
[MIT license](LICENSE-MIT), at your option.

`Mosh` identifies the protocol and upstream software with which this project
seeks interoperability. This repository is not an official Mosh project.
