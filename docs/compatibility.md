# Compatibility contract

## Baseline

The initial oracle is an unmodified stock `mosh-server` 1.4.0. Interoperability
must be demonstrated through public behavior and black-box fixtures.

The first supported path is:

```text
external SSH bootstrap
  -> validated MOSH CONNECT result
  -> IPv4 fixed UDP endpoint
  -> authenticated client session
  -> interactive shell state
```

## Presentation baseline

The first presentation target is a UTF-8 VT-compatible terminal surface. The
library emits synthesized VT paint bytes as bounded chunks that consumers write
in order. A replaced surface explicitly requests a self-contained full repaint;
the initial contract has no display-revision or rendering-acknowledgement layer.

The first stock-server terminal profile is `TERM=xterm-256color`,
`LANG=C.UTF-8`, 24 rows, and 80 columns. Resize is supported within the hard
bounds in [limits](limits.md). Other locales, ambiguous-width policies, and
terminal identities need their own fixture before the claim expands.

The first embedding oracle is LeanTTY's version-locked xterm.js surface. An
independent permissively licensed terminal model provides a second local check.
This baseline does not claim compatibility with every terminal emulator or
native cell renderer.

The public Session supports the standard `Adaptive`, `Always`, and `Never`
prediction display modes. `Adaptive` is the default. All three retain the same
confirmed-epoch ASCII eligibility and authoritative convergence; experimental,
overwrite, Unicode, paste, backspace, and control-sequence prediction remain
outside the compatibility claim.

Mosh synchronizes visible terminal state, not complete shell history. Tests
must record local scrollback behavior, recovery effects, and limitations without
claiming SSH-equivalent or persistent history.

The output stream also does not preserve a remote application's
alternate-screen entry or exit boundary. A visible full-screen frame and an
explicit repaint remain usable, but the consumer projection need not carry an
alternate-screen flag. Consumers own any temporary terminal page used to
isolate the whole Mosh Session from pre-existing local contents.

## Required evidence

Before the compatibility claim expands, tests must cover:

- valid, malformed, ambiguous, and oversized bootstrap output;
- correct and incorrect keys;
- valid, malformed, duplicate, replayed, reordered, delayed, and lost packets;
- client source-address changes without changing the fixed server endpoint or
  losing session state;
- graceful local close, authenticated remote close, cancellation, timeout,
  server disappearance, and late packets;
- resize, Unicode, wide characters, sustained input, and sustained output;
- shell, tmux, a basic editor, alternate screen, and scrollback behavior.

Every test records the server version, platform, network conditions, expected
behavior, and cleanup result.

The current loopback matrix covers the shell, tmux attach/navigation/detach,
Vim full-screen editing and repaint, ordered sustained input, sustained output,
display backpressure, and cancellation. The interactive shell and repaint
fixture drives the Phase 3 public Session API. Stock Mosh exposes the current
visible Vim frame without requiring the local projection to retain the remote
alternate-buffer flag. The fixture asserts that the live projection and a
replacement projection rebuilt from a full repaint both remain on their local
primary buffer, then converge to the shell after Vim exits. A test-only
loopback relay proves recovery after a
1.5-second bidirectional interruption and UDP source-port change; another test
proves that server disappearance leaves the Session repaintable and
cancellable. A Session fixture verifies remote PTY resize from 80×24
to 100×30 and 60×20, followed by a replacement-surface repaint. Stock 1.4.0
black-box fixtures also cover the reserved close target in both
directions, the peer acknowledgement shape, and the bounded no-ACK wait.
Public reachability now reports recent-contact and recent-reply warnings without
treating silence as close. Physical address change, longer recovery behavior,
and scrollback characterization remain open.

The full evidence matrix, offline-first policy, runtime isolation cases, VT
output contract, and physical LeanTTY acceptance are defined in
[testing](testing.md).

## Deferred compatibility

IPv6, configurable port ranges, broader prediction, locale negotiation, broad
terminal application coverage, recoverable local UDP send errors, configurable
reachability thresholds, and non-LeanTTY consumers require separate evidence. ProxyJump applies
only to an embedding application's SSH bootstrap; it does not imply UDP
reachability.
