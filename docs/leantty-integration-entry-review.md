# LeanTTY integration entry review

> Review date: 2026-08-30
>
> Scope: the evidence required before LeanTTY product integration begins

## Decision

Phase 4 product integration is not authorized yet.

The library is technically linkable with LeanTTY's current Rust native
dependencies for ARM64 HarmonyOS. Official HarmonyOS documentation also exposes
UDP sockets, default-network callbacks, socket-to-network binding, and
long-running-task APIs. These facts remove two feasibility risks, but they do
not prove that a normal third-party HarmonyOS PC application preserves and
recovers a Mosh session across backgrounding, lock, sleep, or network changes.

LeanTTY must therefore finish its existing Mosh entry gate before product code
is added. The remaining work is a disposable app-level UDP/lifecycle probe and
a measured comparison with current SSH behavior. If that evidence does not
show material user value, Phase 4 stops without adding a Mosh path.

## Governing boundary

This review applies the established ownership split:

- LeanTTY owns Host resolution, host verification, authentication, controlled
  `mosh-server` startup, Pane lifecycle, and the Terminal Surface.
- `mosh-client` owns the untrusted bootstrap parser, authenticated UDP protocol,
  bounded synchronization, instruction decoding, and public Session lifecycle.
- The first integration is one Pane-owned Mosh Session connected to the
  existing Terminal Surface. It does not add a generic Transport abstraction.
- LeanTTY-specific UI, ArkTS, N-API, Pane, and platform policy stay outside this
  crate.

## Evidence already obtained

### Library and dependency readiness

Phase 3F closed the implementation and security gate while retaining
`publish = false`; see the
[security and publication review](security-publication-review.md). The public
Session contract and its cancellation, output, resize, recovery, and secret
handling behavior are covered by the retained verification suite.

A disposable combined `cdylib` linked `mosh-client` with the dependency families
used by LeanTTY's native layer, including `napi-ohos`, Tokio, Russh, and
Russh-SFTP. Cargo resolved 219 packages and completed an
`aarch64-unknown-linux-ohos` debug build. The resulting 4,508,320-byte artifact
was an ELF64 AArch64 shared object. Its debug size is diagnostic evidence, not a
product size estimate.

The combined graph introduced no second Tokio. Duplicate versions were confined
to transitive packages already selected by the surrounding dependency graph.
The first attempt exposed a stale user-level linker path from an older checkout;
the successful run used LeanTTY's existing OHOS linker and archiver settings for
that process. No global configuration or project source was changed.

This proves source-level coexistence and target linking only. The disposable
build was not installed, launched, or exercised on a device.

### HarmonyOS platform capability

The official platform documentation establishes these available mechanisms:

- applications can observe
  [default-network changes](https://developer.huawei.com/consumer/en/doc/harmonyos-guides/ide_listen-default-network-change)
  and move transmission to the new network;
- the native network API includes default-network callbacks and
  [`OH_NetConn_BindSocket`](https://developer.huawei.com/consumer/en/doc/harmonyos-references-V14/_net_connection-V14);
- HarmonyOS exposes
  [continuous-task support](https://developer.huawei.com/consumer/en/doc/harmonyos-guides/long-time-task-overview)
  for long-lived work such as listening on a socket port; and
- applications can bind UDP sockets to selected networks through the documented
  [multi-network APIs](https://developer.huawei.com/consumer/en/doc/harmonyos-guides/networkboost-netmultipath-network-turbo).

The same official material warns that applications can be suspended or
terminated in the background without an appropriate task, and documents
[`TASK_KEEPING`](https://developer.huawei.com/consumer/en/doc/harmonyos-references-V5/js-apis-resourceschedule-backgroundtaskmanager-V5)
for 2-in-1 devices. Background resource guidance also describes disconnecting
[TCP and UDP when an application has no long-running task](https://developer.huawei.com/consumer/cn/doc/harmonyos-guides-V5/standard-background-hardware-V5),
but its stated device scope does not establish a HarmonyOS PC guarantee.

The documentation therefore supports a feasibility hypothesis, not the product
claim. It cannot replace a signed-HAP test on the target PC.

### Existing environment evidence

The available fixed-endpoint HSL-to-WSL UDP test establishes that the local
network can carry, block, and restore UDP traffic. It does not exercise a
LeanTTY process, application sandbox, Pane lifecycle, HarmonyOS network callback,
lock, sleep, or address change. Earlier device-side Toybox `nc` attempts failed
before product behavior was involved and remain ineligible as acceptance
evidence.

A fresh device-control preflight passed on 2026-08-30. The connected physical PC
is available for a focused probe, but no trustworthy named Mosh product scenario
or product HAP exists yet.

## Remaining entry gate

LeanTTY owns and must record the following evidence before this repository marks
any Phase 4 integration item complete:

1. Measure current SSH behavior on the same physical PC for normal use, brief
   interruption, Wi-Fi or address change, lock, sleep, recovery, cancellation,
   and Pane close. Record visible correctness and recovery, not only timing.
2. If current tools cannot answer the platform question, build one disposable
   signed HAP probe that uses the same native/runtime boundary intended for the
   product. Verify UDP receive/send, default-network change, lock/background,
   sleep/wake, cancellation, and cleanup.
3. Keep the probe out of the product architecture. Remove it after retaining
   commands, versions, results, and failure classification.
4. Compare the measured recovery and correctness value with added dependency,
   lifecycle, platform-policy, security, and maintenance cost.
5. Record an explicit continue or cancel decision in LeanTTY's active work list.

Do not infer success from buildability, SSH/SFTP lifecycle tests, a host-to-host
UDP exchange, or official API availability. A failed probe is also useful: if
ordinary third-party UDP cannot recover reliably without disproportionate
platform policy, cancel the integration rather than add workarounds.

## Conditions after a continue decision

If LeanTTY records a continue decision, implement the smallest vertical slice:

1. reuse its Host, verification, authentication, and secret-handling policy to
   start the stock `mosh-server` and parse the bootstrap result;
2. create one Pane-owned Mosh Session with explicit cancellation and generation
   isolation;
3. feed bounded ordered VT bytes and explicit full repaint into the existing
   Terminal Surface;
4. route input and resize directly through that Session without introducing a
   generic transport interface; and
5. run shell, tmux, editor, interruption, address-change, lock, sleep, UDP-block,
   recovery, cancellation, and Pane-close acceptance on the physical PC.

Before a committed cross-repository dependency is chosen, the maintainer must
also select a stable source and version identity for this crate. The current
`0.0.0`, `publish = false`, and absent repository metadata are acceptable for a
local feasibility build, not for a reproducible product dependency.
