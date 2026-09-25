# Changelog

## Unreleased

## 0.1.2

- Preserve valid U+FFFD and other printable Unicode from stock Mosh terminal
  updates without treating the replacement character as a protocol failure.
- Reject DEL explicitly instead of allowing it to disappear from a terminal
  update. Keep malformed UTF-8 and unsupported control failures explicit.
- Verify selected Latin, CJK, emoji, combining, and replacement text through
  the public Session against stock `mosh-server` 1.4.0.

## 0.1.1

- Preserve an established Session when HarmonyOS temporarily reports
  `PermissionDenied` from UDP I/O. Send plans remain uncommitted, receive
  polling is rate-limited, and the same socket and protocol state resume only
  after authenticated peer progress.
- Keep initial connection failures and unlisted I/O categories explicit. The
  public API, dependency versions, and resource limits are unchanged.

The library suite verifies both I/O directions with deterministic injected
errors. LeanTTY's physical lid-close run followed an application-process
replacement path, so it does not prove same-Session or same-PTY recovery from
the observed `PermissionDenied` path.

See [the 0.1.1 release review](docs/releases/0.1.1.md) for evidence and
publication status.

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
