# Stock 1.4.0 local-prediction fixture

- Date: 2026-08-30
- Oracle: unmodified Ubuntu `mosh-server` 1.4.0
- Environment: Ubuntu 26.04 WSL x86-64, IPv4 loopback
- Terminal: `TERM=xterm-256color`, `LANG=C.UTF-8`, 80 columns, 24 rows
- Remote service: none

## Scenarios

`stock_1_4_0_echo_acknowledges_the_controlled_input_operation` sends an
independently encoded initial resize as client state 1, then one printable byte
as client state 2. Authenticated stock output contains transport
acknowledgement 2 and terminal echo acknowledgement 2.

`stock_1_4_0_prediction_reduces_measured_interactive_echo_latency` starts
separate stock servers and Sessions for the non-predictive baseline and the
predictive path. A test-only UDP relay applies a fixed delay in both directions.
The fixture sends `latencyprobe` one byte at a time and measures from Session
input submission until each character is visible in an independent VT
projection.

| One-way delay | Baseline samples, ms | Prediction samples, ms |
| ---: | --- | --- |
| 0 ms | 28, 27, 23, 26, 27, 23, 26, 27, 22, 27, 28, 23 | 27, 28, 21, 27, 27, 21, 27, 27, 23, 26, 27, 22 |
| 40 ms | 109, 107, 109, 107, 107, 108, 108, 109, 108, 108, 110, 108 | 109, 109, 108, 0, 0, 0, 0, 0, 0, 0, 0, 0 |
| 80 ms | 189, 187, 255, 189, 252, 188, 253, 188, 252, 189, 251, 188 | 189, 190, 249, 0, 0, 0, 0, 0, 0, 0, 0, 0 |

After measurement, each Session sends a control input and a marker command.
The fixture waits for `LATENCY_AUTHORITY_CONVERGED`, then cancels the Session
and terminates the detached stock server.

## Established behavior

- Stock echo acknowledgement identifies the controlled client state.
- The relay affects both directions; every non-predictive sample is at least
  the configured round-trip delay.
- Conservative prediction leaves the first three delayed samples authoritative
  in this run, then removes the network delay from the visible hot epoch.
- Predicted input still reaches and converges with the stock server.

## Limits

The result measures the private Rust Session and VT projection on loopback. It
does not include LeanTTY, N-API, ArkTS, WebView rendering, a physical device,
real WAN jitter, loss, reordering, Unicode prediction, paste, backspace, or
interactive editor prediction. Timing samples are evidence for the relative
gate, not a universal performance guarantee.
