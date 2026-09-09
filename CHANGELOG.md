# Changelog

## Unreleased

- Preserve an established Session when HarmonyOS temporarily reports
  `PermissionDenied` from UDP I/O. Send plans remain uncommitted, receive
  polling is rate-limited, and the same socket and protocol state resume only
  after authenticated peer progress.

## 0.1.0

First release of the independent, unofficial `mosh-client` Rust library.
Requires Rust 1.88 or newer; licensed under MIT OR Apache-2.0.

- Interoperates with stock `mosh-server` 1.4.0 over IPv4 after caller-owned
  SSH authentication, host verification, and server startup.
- Provides one caller-driven asynchronous Session with ordered input, resize,
  bounded VT output, full repaint, authenticated graceful close, and prompt
  cancellation.
- Keeps authenticated transport, replay protection, synchronization, timers,
  terminal state, recovery, and concurrent Session ownership inside the crate.
- Reports reachability separately from lifecycle. Established Sessions survive
  network silence and selected temporary local UDP send errors.
- Supports per-Session `Adaptive` (default), `Always`, and `Never` prediction
  display policies with bounded, confirmed-epoch ASCII prediction.

The initial terminal profile is UTF-8 `xterm-256color`. Output reconstructs the
current visible screen; it does not preserve the remote PTY byte stream,
complete scrollback, or application alternate-screen boundaries. Consumers
own whole-Session terminal isolation and restoration.

IPv6, a CLI or server, SSH implementation, broader prediction, persistent
Sessions across process termination, and broad device/network compatibility
are outside this release. The accepted OCB3 audit and zeroization limitations
are documented in [the security review](docs/security-publication-review.md).

See [the 0.1.0 release review](docs/releases/0.1.0.md) for evidence and
publication status.
