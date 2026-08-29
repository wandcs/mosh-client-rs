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

## Required evidence

Before the compatibility claim expands, tests must cover:

- valid, malformed, ambiguous, and oversized bootstrap output;
- correct and incorrect keys;
- valid, malformed, duplicate, replayed, reordered, delayed, and lost packets;
- peer-address changes without session-state loss;
- cancellation, timeout, server disappearance, and late packets;
- resize, Unicode, wide characters, sustained input, and sustained output;
- shell, tmux, a basic editor, alternate screen, and scrollback behavior.

Every test records the server version, platform, network conditions, expected
behavior, and cleanup result.

## Deferred compatibility

IPv6, configurable port ranges, prediction modes, locale negotiation, broad
terminal application coverage, and non-LeanTTY consumers require separate
evidence. ProxyJump applies only to an embedding application's SSH bootstrap;
it does not imply UDP reachability.
