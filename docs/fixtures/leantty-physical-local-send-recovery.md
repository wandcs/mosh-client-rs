# LeanTTY physical local-send recovery evidence

- Date: 2026-09-02
- Consumer: LeanTTY native Mosh owner using `mosh-client-rs` `e1346b3`
- Consumer artifact SHA-256:
  `2d3c43605ac2c9133387d4c1846d95e120969100f92744528d47db95649afc5a`
- Device: HAD-W32 physical ARM64 HarmonyOS PC
- Peer: unmodified stock `mosh-server` 1.4.0 in a controlled PTY
- Network: fixed IPv4 endpoint and UDP port over the device's real WLAN

## Interface-outage result

The fixture established an interactive Session and completed a baseline shell
command. It then disabled WLAN through the HarmonyOS system status panel and
verified that `wlan0` was off and had lost its IPv4 address. USB HDC remained
only as the test-control and observation channel.

The Session returned a local UDP I/O error after about 7.4 seconds instead of
remaining active and reporting `Interrupted(NoRecentContact)`. LeanTTY closed
the Mosh page. At the failure point, the application process, stock server, and
controlled remote PTY were still alive. The result was therefore not process
reclamation, remote shell exit, authenticated peer close, or a UI warning
promoted to an error.

This proves that propagating every established-Session UDP send error as fatal
can destroy a Session during a temporary physical interface transition. It
does not prove that every local I/O error is recoverable.

LeanTTY evidence identifier:
`device-mosh-wifi-pause-recovery-20260902-retry2/device-mosh.json`.

## Retained limitation and closing gate

The sanitized artifact retained the stable `local-udp-io-error` category but
not the precise Rust `ErrorKind`. ADR 0011 therefore admits only the narrow
standard categories whose documented meanings are interface, network route,
host route, or usable local-address loss. Broad and platform-specific errors
remain fatal.

LeanTTY must pin the fixed revision and repeat the same physical toggle. The
closing run must observe `Interrupted`, restore WLAN, observe `Responsive`, and
execute a new exact command in the same remote PTY. If HarmonyOS reports only a
broad error category, that exact safe category or OS code must be captured
before the allowlist expands.

## Retained data

This record contains no session key, credential, host address, packet capture,
terminal transcript, or user data. Detailed device logs remain in LeanTTY's
local verification area and are not a runtime or release dependency of this
crate.
