# Protocol provenance

This log protects the independent implementation boundary. Add an entry before
or with every nontrivial compatibility rule.

## Allowed evidence classes

- public protocol documentation, papers, and standards;
- public command and bootstrap contracts;
- black-box observations from unmodified released binaries;
- packet captures produced by project-controlled fixtures with secrets removed;
- original experiments and analysis written for this project.

GPL-covered source code, comments, file organization, and tests are not project
inputs and must not be copied or translated.

## Entry template

```text
Date:
Behavior:
Evidence class:
Source or fixture:
Observed versions:
Implementation consequence:
Limits and uncertainty:
Author:
```

## Initial entries

### Project compatibility baseline

- Date: 2026-08-29
- Behavior: SSH starts `mosh-server`; the interactive session then uses UDP.
- Evidence class: public command contract and project-controlled environment
  experiment.
- Observed version: stock `mosh-server` 1.4.0.
- Implementation consequence: this crate receives validated bootstrap data and
  does not own SSH authentication or host verification.
- Limits: this entry does not define the packet or synchronization protocol.

### Fixed-port UDP environment feasibility

- Date: 2026-08-29
- Behavior: a physical ARM64 HarmonyOS PC reached a stock server on a fixed UDP
  port over a wired LAN; stable firewall allow, block, and restore states were
  distinguishable.
- Evidence class: project-controlled black-box environment experiment.
- Observed endpoint: `192.168.1.4:60042/UDP`; the address was test-local and is
  not a product default.
- Implementation consequence: a fixed IPv4 UDP endpoint is a viable first
  integration target.
- Limits: the experiment did not prove HarmonyOS application lifecycle recovery,
  address roaming, terminal synchronization, or product value.
