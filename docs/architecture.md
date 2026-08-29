# Architecture

## Ownership

The client session owns one protocol lifecycle:

```text
validated bootstrap
  -> session key and UDP endpoint
  -> authenticated packet transport
  -> sequence, replay, and roaming state
  -> state synchronization and prediction
  -> structured terminal updates
```

The embedding application owns SSH authentication, host verification,
`mosh-server` startup, UI state, and terminal presentation. These responsibilities
remain outside this crate.

## Initial modules

Modules should appear only when implementation begins and a testable boundary
exists. Expected responsibilities are:

- bootstrap parsing and validation;
- authenticated packet encoding and decoding;
- sequence, replay, timeout, and peer-address state;
- state synchronization and local prediction;
- terminal state and structured rendering changes;
- session orchestration and cleanup.

These are responsibilities, not a commitment to six public modules or crates.
The public API remains small and session-oriented.

## Platform boundary

Protocol logic should use portable Rust. OS networking and timers require a
small explicit boundary so tests can inject loss, duplication, reordering,
delay, address changes, and cancellation. Do not introduce a general transport
framework.

## Security invariants

- Validate all lengths before allocation or decoding.
- Authenticate packets before processing their payload.
- Reject replay and invalid sequence transitions.
- Bound buffers, retries, timers, prediction, and terminal state.
- Zeroize session secrets where the selected audited primitive supports it.
- Never expose secrets through `Debug`, errors, traces, or terminal updates.
- Make cancellation and drop release sockets, timers, buffers, and secrets.
