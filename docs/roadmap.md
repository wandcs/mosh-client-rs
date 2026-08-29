# Roadmap

This file is the only active work list. Complete phases in order. A later phase
does not authorize work before its entry gate passes.

## Phase 0: Freeze the implementation contract

- [ ] Identify public specifications, papers, standards, and black-box fixtures
  needed for the initial wire contract.
- [ ] Record unresolved protocol gaps without consulting or copying GPL source.
- [ ] Select and audit candidate crates for authenticated encryption, random
  generation, zeroization, serialization, timers, and Unicode behavior.
- [ ] Define packet, buffer, retry, timeout, and terminal-state limits before
  decoding untrusted data.
- [ ] Freeze the first compatibility claim: stock `mosh-server` 1.4.0, IPv4,
  fixed UDP port, external SSH bootstrap, and client-only library.

## Phase 1: Build the authenticated UDP core

- [ ] Implement strict bootstrap value parsing without logging secrets.
- [ ] Implement packet encoding, authentication, decoding, sequence handling,
  replay rejection, and bounded errors with audited primitives.
- [ ] Build a deterministic network fixture for loss, duplication, reordering,
  delay, corruption, timeout, cancellation, and peer-address changes.
- [ ] Interoperate with the stock server for the smallest authenticated exchange.
- [ ] Stop if the protocol cannot be implemented independently, safely, or with
  permissively licensed maintained dependencies.

## Phase 2: Implement synchronization and terminal state

- [ ] Define the synchronization state machine and bounded transition model.
- [ ] Implement acknowledgement, retransmission, roaming, and recovery behavior.
- [ ] Implement terminal state, local prediction, resize, Unicode, and structured
  rendering changes without binding to one UI toolkit.
- [ ] Prove shell, tmux, editor, alternate-screen, and sustained-I/O behavior
  against the stock server.
- [ ] Stop if terminal correctness or recovery remains materially below SSH.

## Phase 3: Stabilize the library contract

- [ ] Expose one small session-oriented API with explicit state and cancellation.
- [ ] Prove two concurrent sessions do not share keys, packets, terminal state,
  timers, errors, or cleanup.
- [ ] Add fuzzing and resource-exhaustion coverage for parsers and state machines.
- [ ] Audit dependency licenses, supply chain, `unsafe` code, and secret handling.
- [ ] Decide whether the crate is ready to publish; keep `publish = false` until
  the decision is recorded.

## Phase 4: Integrate one LeanTTY vertical slice

- [ ] Keep LeanTTY responsible for Host resolution, host verification,
  authentication, and controlled server startup.
- [ ] Connect one Pane-owned Mosh Session to one Terminal Surface without a
  generic Transport plugin layer.
- [ ] Build for ARM64 HarmonyOS and verify a real shell, tmux, and basic editor.
- [ ] Compare SSH and Mosh under normal network, interruption, address change,
  lock, sleep, UDP block, recovery, cancellation, and Pane close.
- [ ] Integrate only if measured recovery, correctness, security, and maintenance
  value clearly exceed the added complexity.

## Out of scope

- Mosh server implementation or distribution.
- Server installation and lifecycle management.
- File transfer, port forwarding, VPN behavior, or session sharing.
- Generic transport plugins or a framework for hypothetical protocols.
- Claims of mobile, IPv6, ProxyJump UDP, or broad terminal compatibility without
  their own evidence gates.
