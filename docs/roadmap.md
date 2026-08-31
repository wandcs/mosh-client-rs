# Roadmap

This file is the only active work list. Complete phases in order. A later phase
does not authorize work before its entry gate passes. Every item must follow
[the project principles](project-principles.md); completing a checklist does not
justify violating them.

## Phase 0: Freeze the implementation contract

- [x] Identify public specifications, papers, standards, and black-box fixtures
  needed for the initial wire contract.
- [x] Record unresolved protocol gaps without treating stock or third-party
  source as authoritative protocol evidence.
- [x] Select and audit candidate crates for authenticated encryption, random
  generation, zeroization, serialization, timers, and Unicode behavior.
- [x] Define packet, buffer, retry, timeout, and terminal-state limits before
  decoding untrusted data.
- [x] Freeze the first compatibility claim: stock `mosh-server` 1.4.0, IPv4,
  fixed UDP port, external SSH bootstrap, and client-only library.

Phase 0 is complete. [ADR 0004](decisions/0004-rustcrypto-ocb3-security-exception.md)
accepts a bounded OCB3 security exception. Candidate selection did not replace
the lockfile, license, advisory, Linux, and ARM64 HarmonyOS checks performed in
Phase 1.

## Phase 1: Build the authenticated UDP core

- [x] Implement strict bootstrap value parsing without logging secrets.
- [x] Implement packet encoding, authentication, decoding, sequence handling,
  replay rejection, the fragment envelope codec, and bounded errors with
  audited primitives.
- [x] Build a deterministic network fixture for loss, duplication, reordering,
  delay, corruption, timeout, cancellation, and peer-address changes.
- [x] Interoperate with the stock server for the smallest authenticated exchange.
- [x] Evaluate the stop condition: current evidence supports an independent,
  bounded implementation with permissively licensed maintained dependencies.

Phase 1 is complete as of 2026-08-29. Stable and Rust 1.85 formatting, Clippy,
tests, rustdoc, license policy, RustSec audit, all local stock 1.4.0 fixtures,
and the ARM64 HarmonyOS library build passed. The first authenticated exchange
does not yet constitute a working shell Session; synchronization and terminal
semantics remain Phase 2 work.

## Phase 2: Implement synchronization and terminal state

- [x] Define the synchronization state machine and bounded transition model.
- [x] Implement deterministic acknowledgement, retransmission, heartbeat,
  roaming, and recovery scheduling.
- [x] Implement bounded fragment reassembly with the smallest concurrency
  policy that passes stock-server loss and reordering fixtures.
- [x] Implement terminal state, resize, Unicode, and deterministic ordered VT
  output without binding to one UI toolkit.
- [x] Move large private unit and stock fixtures into module-local test files
  without changing production boundaries or public behavior.
- [x] Implement a private non-predictive Session driver and prove one stock
  shell, input, output, full-repaint, and cancellation loop without freezing
  the public API.
- [x] Prove the non-predictive client is usable, then add bounded local
  prediction only if measured latency shows a material interaction benefit.
- [x] Prove shell, tmux, editor, alternate-screen, and sustained-I/O behavior
  against the stock server.
- [x] Prove local stock Session recovery after a bidirectional outage and
  authenticated UDP source-port change, plus controllable server disappearance.
- [x] Evaluate the stop gate: supported terminal behavior and local recovery
  do not remain materially below the SSH baseline, so continue.

Phase 2 is complete as of 2026-08-30. Stock 1.4.0 fixtures cover a shell,
remote PTY resize, tmux, Vim, sustained input and output, display backpressure,
explicit repaint, controlled latency, a bidirectional outage, UDP source-port
change, server disappearance, and cancellation. [ADR 0006](decisions/0006-phase-2-core-viability-gate.md)
records why this evidence admits Phase 3 without expanding the compatibility
claim or replacing the later LeanTTY device comparison.

## Phase 3: Stabilize the library contract

- [x] Expose one small session-oriented API with explicit state and cancellation.
  [ADR 0007](decisions/0007-public-session-api.md) records the split between
  latest lifecycle state, ordered VT output, and prompt cancellation.

Complete the remaining Phase 3 groups in order. Read-only evidence collection
may overlap baseline verification, but a later group does not authorize early
refactoring or behavior changes.

### Phase 3A: Freeze the baseline and prove isolation

- [x] Record a reviewable Phase 3 baseline revision after the existing Phase 2
  and public-API checks pass, so each later refactor has a known comparison and
  rollback point.
- [x] Prove through the public API that two interleaved sessions do not share
  keys, endpoints, packets, input, VT output, terminal state, timers, errors,
  cancellation, or cleanup.
- [x] Cancel and drop each session independently; prove the other session keeps
  running and late events cannot enter a closed or replacement session.

Phase 3A is complete as of 2026-08-30. Revision `288ce58` is the comparison and
rollback baseline. A local stock 1.4.0 fixture now proves public-API isolation
across two active Sessions and a replacement lifecycle, including wrong-key
ciphertext, independent terminal state and timers, cancellation, owner drop,
and late-packet rejection.

### Phase 3B: Collect external maintenance evidence

- [x] Survey MoshCatty, `dart_mosh`, and `mosh-go` pull requests, issues,
  releases, and important commits from the latest 24 months. Extend to the full
  history when recent activity is too limited to reveal maintenance patterns.
- [x] Classify the changes by protocol compatibility, synchronization and
  recovery, terminal behavior, lifecycle and cancellation, performance and
  resource use, testing, and maintenance-only work.
- [x] For relevant historical bugs, record the root cause, affected path,
  observable failure, fix pattern, regression-test layer, applicability to this
  crate, and confidence. Record recurring cross-project patterns separately
  from project-specific behavior.
- [x] Treat third-party code as a source of hypotheses and design alternatives.
  Derive applicable tests independently, verify wire rules with permitted
  primary evidence and stock fixtures, and update
  [provenance](provenance.md) for every nontrivial compatibility rule.

Phase 3B is complete as of 2026-08-30. The
[external maintenance evidence](external-maintenance-evidence.md) covers each
repository's full default-branch, tracker, and release history, separates
accepted facts from low-confidence reports, and defines independent test
candidates. No new wire rule was accepted, so no protocol-provenance entry was
added.

### Phase 3C: Review necessity before code structure

- [x] Review each major mechanism against the project principles, protocol
  contract, paper and specifications, measured behavior, stock fixtures, and
  Phase 3B evidence. Use the paper as design evidence, not a completeness
  requirement. Classify the mechanism as **keep**, **simplify**, **remove**, or
  **defer**; record its user value, evidence, resource cost, maintenance cost,
  compatibility risk, and rollback.
- [x] Cover the Session driver and wake path; lifecycle, cancellation, and
  backpressure; `ClientHistory` and SSP checkpoints; retained terminal state;
  fragmentation; repaint and prediction; timers and recovery; and the public
  event surface.
- [x] Prefer the smallest design that preserves the supported Mosh core path.
  Do not preserve paper-complete behavior, abstractions, caches, state, or
  branches without a current contract or demonstrated interoperability need.
- [x] Revise the governing decision or technical document before changing code
  when the review conflicts with an accepted design. Require independent
  protocol evidence and the smallest stock fixture for wire changes.

Phase 3C is complete as of 2026-08-30. The
[mechanism necessity review](necessity-review.md) keeps the single-owner Session,
bounded lifecycle and output paths, SSP operation history and checkpoints,
retained terminal snapshots, fragmentation, repaint, the measured prediction
subset, and deterministic recovery timers. It admits only behavior-preserving
private simplification in Phase 3D. Remote clean exit, selected temporary UDP
send recovery, reachability events, broader prediction, public cells, and
overlapping fragment messages remain behind their evidence gates.

### Phase 3D: Refactor only the retained design

Phase 3D is complete as of 2026-08-30. The
[refactoring log](refactoring-log.md) records each accepted change and retained
boundary. The work unified private Session closure, isolated client operation
history, kept terminal and measured-prediction ownership unchanged, simplified
single-message reassembly, made scheduler generation failure atomic, and
consolidated duplicated Linux stock-fixture support. Parser, packet, codec,
terminal, recovery, and public API boundaries remain unchanged where review
found no lower-cost design.

- [x] Review retained code for mixed ownership, mixed responsibilities, long or
  deeply nested functions, duplicated logic, redundant state, avoidable clones
  and allocations, oversized error or state variants, and test-helper reuse.
  Split code at ownership, lifecycle, invariant, or reusable-test boundaries;
  line count alone does not justify a new abstraction.
- [x] Record each accepted refactor with its expected simplification, preserved
  behavior, smallest characterization or regression test, verification path,
  and rollback. Reject changes whose maintenance cost exceeds their measured
  value.
- [x] Apply accepted changes in this order: Session lifecycle, cancellation,
  and concurrent ownership; SSP, `ClientHistory`, and terminal snapshots;
  terminal, repaint, and prediction; parser, fragmentation, and timing only
  where the review found a concrete need; then shared test support.
- [x] Keep each batch reviewable and run its focused tests and smallest relevant
  stock fixture before starting the next batch.

### Phase 3E: Rebuild tests around retained risks

Phase 3E is complete as of 2026-08-30. The
[coverage map](test-coverage.md) connects retained risks to unit, boundary,
property, deterministic, concurrency, fuzz, and stock evidence. It records
external defect regressions, known stock limitations, compatibility exclusions,
and the Phase 4 and 3F gates without changing production behavior or public API.

- [x] Build a coverage map from supported core behavior and applicable external
  bug classes to unit, boundary, property, deterministic state-machine,
  concurrency, fuzz, and stock-interoperability tests. Mark gaps, redundant
  coverage, and behavior outside the compatibility claim.
- [x] After production boundaries stabilize, reorganize tests by risk: protocol
  and parser boundaries; SSP and deterministic timing; Session concurrency and
  lifecycle; terminal, repaint, prediction, and convergence; then stock-only
  behavior that local models cannot prove.
- [x] Add independently designed regression tests for applicable third-party
  bugs. Preserve unique local fixtures, and remove duplicate or obsolete tests
  only after the coverage map proves that no supported behavior is lost.
- [x] Add fuzzing and resource-exhaustion coverage for the retained parsers and
  state machines, using the explicit limits and invariants established by the
  preceding reviews.

### Phase 3F: Close security and publication gates

Phase 3F is complete as of 2026-08-30. The
[security and publication review](security-publication-review.md) records the
resolved dependency and `unsafe` boundaries, two secret-lifetime fixes, the
complete verification evidence, the accepted OCB3 residual risk, and the
decision to keep publication disabled until release identity and package
metadata are chosen.

- [x] Audit dependency licenses, supply chain, `unsafe` code, and secret handling.
- [x] Re-run the complete verification suite, relevant stock fixtures, and the
  two-session isolation scenarios after the accepted refactors and audit fixes.
- [x] Decide whether the crate is ready to publish; keep `publish = false` until
  the decision is recorded.

## Phase 4: Integrate one LeanTTY vertical slice

The 2026-08-30 [integration entry review](leantty-integration-entry-review.md)
records successful native consumer integration at commit `69450f4`. LeanTTY
reused its SSH security path, established an independent native Mosh owner, and
linked this crate in an ARM64 OHOS release build. The library core is frozen at
that consumer milestone. Product integration remains open until LeanTTY connects
its Pane and Terminal Surface and completes physical Session and recovery tests.

LeanTTY also reproduced a library lifecycle defect: local cancellation did not
perform the stock close exchange, and authenticated remote logout did not end
the public Session. [ADR 0008](decisions/0008-authenticated-graceful-close.md)
reopened that bounded API/protocol work. Stock 1.4.0 black-box fixtures now
establish both directions and the close timeout; the additive graceful-close
API preserves hard cancellation, ordinary outage recovery, and the existing
public lifecycle states.

- [x] Independently establish the stock 1.4.0 close target, acknowledgement,
  retransmission window, and final-difference behavior.
- [x] Add graceful local close and authenticated remote completion without
  weakening cancellation, owner drop, bounds, output drain, or Session
  isolation.

LeanTTY physical testing then reproduced a recoverable bidirectional outage and
an indistinguishable silent server termination. [ADR 0009](decisions/0009-session-reachability.md)
admits one bounded library follow-up:

- [x] Add an independent latest-value reachability contract with recent-contact
  and recent-reply reasons, without changing monotonic lifecycle state.
- [x] Add a 15-second first-attachment timeout that never applies to an active
  Session.
- [x] Prove thresholds, invalid-traffic isolation, recovery, server silence,
  output independence, and stock-server interoperability.

LeanTTY then reproduced MCRS-006: the fixed private predictor made the standard
`adaptive`, `always`, and `never` choices unrepresentable through the public
Session API. [ADR 0010](decisions/0010-public-prediction-modes.md) admits one
bounded library follow-up:

- [x] Add the exact three-value per-Session mode and make `Adaptive` the
  standard `Session::connect` default without adding global or runtime policy.
- [x] Keep the existing confirmed-epoch ASCII predictor and implement only a
  thin RTT/glitch display gate with recorded bounds.
- [x] Prove default selection, all modes, concurrent isolation, adaptive
  thresholds, resource bounds, and stock-server convergence.

A first LeanTTY ARM64 run used `stty -echo` and could not prove that stock echo
acknowledgement had confirmed the epoch. A project-owned follow-up therefore
used normal stock PTY echo and proved that a confirmed public `Always` epoch
survives complete bidirectional loss while `Never` stays authoritative-only.

The corrected LeanTTY ARM64 fixture then used the same public API, normal kernel
echo, a stock interactive shell, and a controlled 40 ms delay in each direction.
Every `Always` warmup byte still waited roughly one round trip. Native Session
output already contained that delay; N-API, ArkTS, and rendering added only a
few milliseconds. Code review and a deterministic failing regression identified
the library defect: any authenticated terminal update without a new echo
acknowledgement cleared the confirmed epoch, even though absence of a repeated
acknowledgement is not a prediction mismatch. The bounded fix preserves an idle
confirmed epoch and an unchanged pending base, while still clearing a pending
projection on actual authoritative divergence.

- [x] Prove through the public Session contract that a confirmed `Always` epoch
  survives complete UDP loss while `Never` stays authoritative-only.
- [x] Reproduce and fix the acknowledgement-free epoch reset with deterministic
  state-machine coverage and the existing stock latency and full-loss fixtures.
- [ ] Pin the fixed revision in LeanTTY and prove on ARM64 that at least one
  post-confirmation `Always` byte is visible before the 40 ms one-way delay.

### Phase 4 code-organization maintenance

Repeated consumer-driven additions have made two existing file boundaries
harder to review. This user-authorized maintenance may move private code and
tests, but it must not change protocol behavior, the public API, production
ownership, or the one-crate design.

- [x] Split the Session stock recovery tests by prediction, recovery, and
  isolation scenario. Give the Session-specific UDP relay one test-only owner;
  do not introduce a generic transport or universal stock fixture.
- [x] Separate the public Session facade from its private driver runtime while
  preserving the single Session owner, event priority, shutdown order, and
  existing module API.
- [x] Run focused checks after each move, then run formatting, Clippy, and all
  tests in the default WSL distribution before closing this maintenance batch.

This maintenance batch is complete as of 2026-08-31. The moved stock recovery
fixture and six Session state-machine tests passed. The final formatting,
Clippy, and all-target test gates also passed without a production behavior or
public API change.

- [x] Keep LeanTTY responsible for Host resolution, host verification,
  authentication, and controlled server startup.
- [ ] Connect one Pane-owned Mosh Session to one Terminal Surface without a
  generic Transport plugin layer.
- [ ] Build for ARM64 HarmonyOS and verify a real shell, tmux, and basic editor.
- [ ] Compare SSH and Mosh under normal network, interruption, address change,
  lock, sleep, UDP block, recovery, cancellation, and Pane close.
- [ ] Integrate only if measured recovery, correctness, security, and maintenance
  value clearly exceed the added complexity.

Reopen library development only for a reproduced protocol or API defect, an
observed platform recovery rule, a dependency security event, or an explicit
publication review. LeanTTY product work does not by itself authorize new core
features.

## Out of scope

- Mosh server implementation or distribution.
- Server installation and lifecycle management.
- File transfer, port forwarding, VPN behavior, or session sharing.
- Generic transport plugins or a framework for hypothetical protocols.
- Claims of mobile, IPv6, ProxyJump UDP, or broad terminal compatibility without
  their own evidence gates.
