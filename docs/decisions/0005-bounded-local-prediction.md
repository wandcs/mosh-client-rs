# 0005: Bounded local prediction

- Status: Accepted
- Date: 2026-08-30

ADR 0010 later adds a public per-Session display policy. It supersedes only
this decision's original exclusion of prediction modes; the bounded predictor,
confirmed epoch, authority, eligibility, and resource limits remain unchanged.

## Context

Phase 2 required a usable non-predictive Session before admitting local
prediction. A project-controlled stock 1.4.0 fixture measured visible
character echo through a loopback relay. The non-predictive median was 108 ms
with 40 ms delay in each direction and 189 ms with 80 ms in each direction.
That cost is material relative to the 27 ms loopback baseline.

Prediction must not replace authenticated terminal authority, enlarge the
public API, depend on a UI toolkit, or reproduce the stock client's complete
heuristic system.

## Decision

Add one private, bounded prediction projection beside the authoritative
terminal state.

- The first eligible character in an epoch remains hidden until the stock
  server's echo acknowledgement names its client state and the resulting
  authoritative screen exactly matches the expected projection.
- After that confirmation, later single-byte printable ASCII input in the same
  epoch may be painted immediately.
- An authenticated terminal update that carries no newer echo acknowledgement
  does not by itself revoke the confirmed epoch. If a projection is pending,
  an unchanged authoritative base preserves it; an actual display mismatch
  still clears it immediately.
- Control input, escape sequences, backspace, paste, resize, one divergence,
  capacity exhaustion, or age expiry clears the projection and ends the epoch.
- The authoritative terminal state is never mutated by prediction. A matching
  server update replaces the projection without a visible correction; a
  mismatch repaints from authority.
- Prediction is limited to 32 ASCII scalars, 32 bytes, and 10 seconds. Age is a
  resource and stale-display bound, not evidence that a prediction succeeded.
- Pending characters share one base and one projected screen; the queue does
  not retain a full screen per character. An explicit repaint clears the
  projection and emits authority.
- Output remains the existing bounded, ordered VT byte stream. The initial
  implementation added no display acknowledgement, cell API, or
  consumer-specific type. ADR 0010 later adds only the standard three-value
  per-Session prediction display policy.

## Evidence

The stock 1.4.0 black-box fixture independently confirmed that controlled
client state 2 produces both transport acknowledgement 2 and terminal echo
acknowledgement 2. A second fixture compares independent non-predictive and
predictive Sessions under the same relay:

| One-way delay | Non-predictive median | Predictive hot-epoch median |
| ---: | ---: | ---: |
| 0 ms | 27 ms | 27 ms |
| 40 ms | 108 ms | 0 ms |
| 80 ms | 189 ms | 0 ms |

The delayed cases required three authoritative characters before the
conservative epoch became active. Each case then ran a controlled shell command
to prove eventual stock-server convergence. This is a WSL loopback protocol
measurement, not an end-to-end LeanTTY, WebView, or physical-network claim.

The [Mosh paper](https://mosh.org/mosh-paper.pdf) supports confirmed epochs and
server echo acknowledgements rather than treating a client timeout as proof.
[MoshCatty](https://github.com/binaricat/MoshCatty) and
[mosh-go](https://github.com/unixshells/mosh-go) provide independent
architecture comparisons, not wire authority or source material.

## Consequences

One prediction base, one projected screen, and one emitted display snapshot are
retained because the last visible state may differ from SSP history. The
authoritative SSP snapshot set remains unchanged and bounded separately.

The first character after an epoch reset still pays network latency. Unicode,
backspace, pasted text, control sequences, and speculative editing remain
unpredicted. Extend eligibility only after a separate correctness and value
case; rollback consists of removing the private projection while preserving
the Session and VT contracts.
