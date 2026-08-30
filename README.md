# mosh-client-rs

An independent, unofficial, wire-compatible Mosh client implementation in Rust.

## Status

Phase 1's bounded authenticated UDP core and Phase 2 are complete. Phase 3 now
exports the first small Session API with explicit lifecycle state, distinct
graceful close and prompt cancellation, bounded commands, and ordered VT
output. A separate latest-value reachability contract reports missing recent
contact or reply without treating network silence as close. Phase 2
includes SSP synchronization, timing and recovery scheduling, bounded fragment
reassembly, authoritative terminal state, VT painting, and a Session
driver with bounded confirmed-epoch ASCII prediction. A local stock
`mosh-server` 1.4.0 fixture completed an interactive shell exchange from
bootstrap through prompt, input, output, full repaint, and cancellation.
Additional loopback fixtures cover tmux attach, two-window
navigation, detach and reattach, a Vim full-screen edit and repaint, 128 ordered
input commands, 1,200 lines of output under display backpressure, and recovery
after a 1.5-second bidirectional outage plus an authenticated UDP source-port
change. A separate fixture proves that server disappearance leaves the Session
locally repaintable and cancellable. A stock resize fixture verifies remote PTY
changes from 80×24 to 100×30 and 60×20. A controlled-delay comparison records
non-predictive medians of 108 ms and 189 ms at 80 ms and 160 ms imposed RTT,
while the confirmed prediction epoch removes that visible network delay and
still converges to a stock-server marker.

The Phase 2 viability gate found no material terminal-correctness or recovery
deficit within the declared local compatibility scope. Public contract tests
cover validation, owner shutdown, state, graceful close, and idempotent
cancellation; stock 1.4.0 fixtures drive the same public API and verify both
authenticated close directions. LeanTTY has also integrated
the crate behind an independent native Mosh owner and linked it in an ARM64 OHOS
release build without adding a generic Transport layer. Pane, Terminal Surface,
command-entry, and physical Session gates remain open in LeanTTY.

The Phase 3F security and packaging review keeps the crate unpublished until the
maintainer chooses a release version, immutable tag, and complete package
metadata. See [the roadmap](docs/roadmap.md), the
[LeanTTY integration review](docs/leantty-integration-entry-review.md), and the
[security and publication review](docs/security-publication-review.md).

## Library shape

`Session::connect` returns a caller-owned `Session` handle and a `SessionTask`.
The caller runs the task on its own Tokio executor, sends input or resize
commands through the handle, consumes ordered VT chunks, and explicitly closes,
cancels, or drops the Session. Reachability remains independent from lifecycle
and output backpressure. See
[ADR 0007](docs/decisions/0007-public-session-api.md) for lifecycle and
backpressure semantics and
[ADR 0008](docs/decisions/0008-authenticated-graceful-close.md) for the
authenticated close exchange, and
[ADR 0009](docs/decisions/0009-session-reachability.md) for reachability and
initial attachment timeout.

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

The [project principles](docs/project-principles.md) govern scope, architecture,
dependencies, public API, testing, and LeanTTY integration. See also
[architecture](docs/architecture.md), [protocol contract](docs/protocol-contract.md),
[limits](docs/limits.md), [testing](docs/testing.md),
[compatibility](docs/compatibility.md),
[dependency assessment](docs/dependency-assessment.md),
[implementation survey](docs/implementation-survey.md), and [project
decisions](docs/decisions/).

## Independent implementation

This project does not copy GPL-covered Mosh source code, comments, file
organization, or tests. Protocol behavior must come from public specifications,
papers, standards, documented black-box observations, or original analysis.
Every nontrivial compatibility rule must record its source in
[the provenance log](docs/provenance.md).

Third-party implementations, including MoshCatty, `dart_mosh`, and `mosh-go`,
may inform architecture hypotheses and tradeoffs. Project code and tests remain
independently written, and third-party implementations do not replace
independent wire evidence or stock-server interoperability verification.

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
