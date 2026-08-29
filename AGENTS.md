# mosh-client-rs agent instructions

## Authority

Read `README.md`, `docs/decisions/`, `docs/architecture.md`,
`docs/compatibility.md`, `docs/provenance.md`, and `docs/roadmap.md` before
changing protocol behavior, dependencies, public API, or project scope.

`docs/roadmap.md` is the only active work list. Historical experiments and
unrecorded ideas do not authorize implementation.

## Scope

- Build an independent, unofficial Rust Mosh client.
- Interoperate with the stock `mosh-server`; do not implement or bundle a server.
- Keep one crate until a real state, lifecycle, test, or platform boundary
  requires another crate.
- Do not add a CLI, generic transport interface, plugin system, server manager,
  file transfer, or port forwarding without a recorded decision.
- Keep LeanTTY-specific UI, Pane, Session, ArkTS, N-API, and HarmonyOS policy out
  of the protocol crate.

## Licensing and provenance

- New project code is `MIT OR Apache-2.0`.
- Do not copy or translate GPL-covered source code, comments, file organization,
  or tests.
- Record every nontrivial wire or compatibility rule in `docs/provenance.md`.
- Treat upstream binaries as black-box interoperability oracles only.
- Audit every dependency for license, maintenance, security, target support,
  transitive dependencies, and `unsafe` code before adoption.
- Use established cryptographic crates; never implement cryptographic
  primitives from scratch.

## Security and correctness

- Treat bootstrap text, endpoints, datagrams, sequence numbers, terminal state,
  and peer addresses as untrusted input.
- Set explicit limits for every packet, buffer, retry, timer, and parser.
- Keep session secrets in memory only. Never place them in logs, errors, test
  snapshots, environment dumps, preferences, or terminal output.
- Reject replay, malformed authentication data, incompatible versions, and
  ambiguous bootstrap output explicitly.
- Preserve ordering, cancellation, cleanup, and late-event isolation.

## Development and verification

Run Rust formatting, linting, tests, and builds in the default WSL distribution
at `/mnt/c/repos/mosh-client-rs`.

Before claiming completion, run:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Protocol changes also require the smallest relevant interoperability fixture.
A successful build does not prove wire compatibility, recovery, terminal
correctness, or ARM64 HarmonyOS behavior.
