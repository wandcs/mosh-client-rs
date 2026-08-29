# Phase 3 refactoring log

This log records accepted Phase 3D refactors. Each batch must preserve the
[mechanism necessity review](necessity-review.md) and remain independently
reviewable and reversible.

## 3D-1: Unify private Session closure

- **Status:** implemented and verified on 2026-08-30
- **Rollback:** Phase 3C revision `bd4eca0`
- **Problem:** the driver used a private `SessionClose` enum to distinguish
  command-consumer and output-consumer loss, then immediately mapped both to
  public `SessionExit::OwnerDropped`. The wake path represented those closures
  separately, and command handling returned an optional close result even
  though no command could produce one.
- **Change:** make the driver return `SessionExit` directly; map both closed
  owner channels to one private `Wake::OwnerDropped`; make command handling
  return only success or failure; and keep multi-fragment cancellation as an
  uncommitted send followed by the driver's single cancellation decision.
- **Expected simplification:** remove one mirrored lifecycle enum, its
  conversion, two duplicate wake outcomes, and a permanently empty optional
  result. The Session driver remains the sole owner of final lifecycle choice.
- **Preserved behavior:** public API and error types; `Cancelled` versus
  `OwnerDropped`; cancellation priority; command and output ordering; closed
  lifecycle publication; and the rule that a cancelled partial instruction
  never commits SSP or timer state.
- **Characterization:** public `tests/session.rs` covers idempotent cancellation,
  owner drop, closed lifecycle, output closure, and command rejection. The
  private cancellation test covers a driver waiting without a server. The
  stock two-Session isolation fixture covers independent cancellation, owner
  drop, replacement lifecycle, and late-packet rejection.
- **Verification:** focused Session tests, the stock public-isolation fixture,
  and the complete formatting, Clippy, and test suite.
- **Rejected expansion:** replacing the explicit wake poller, moving
  cancellation into the command queue, adding subtasks, or changing remote
  close and local-I/O policy. None is needed to remove the duplicated private
  lifecycle representation.

## 3D-2: Isolate client operation history

- **Status:** implemented and verified on 2026-08-30
- **Rollback:** Phase 3D-1 revision `2711b7c`
- **Review:** keep SSP plan/commit, generations, dispositions, semantic discard
  floors, and reported evictions unchanged. They already form one deterministic
  synchronization boundary. Keep SSP-indexed terminal snapshots in the Session
  driver because it applies terminal differences and consumes synchronization
  commit results; a forwarding wrapper would not remove an invariant.
- **Problem:** `ClientHistory` owns a separate bounded operation log, monotonic
  operation indexes, SSP checkpoints, difference encoding, ACK prefix release,
  and sender-eviction cleanup, but its implementation and tests lived inside
  the Session driver module.
- **Change:** move `ClientHistory` and its focused tests to a private Session
  submodule. Add a characterization proving that an ACK for an older sent state
  cannot remove later input before that input is sent.
- **Expected simplification:** make the cumulative client object and its
  invariants reviewable without exposing it publicly or splitting the crate.
  The Session driver keeps orchestration; the submodule owns operation-history
  representation.
- **Preserved behavior:** operation order and bytes; resize ordering; state
  checkpoints; cumulative differences; ACK release; sender-capacity eviction;
  all existing limits, errors, wire encoding, and public API.
- **Characterization:** the new test passed both before and after the move.
  Existing synchronization tests cover unknown, stale, reordered, and retained
  acknowledgements. The stock sustained-I/O fixture remains the smallest
  end-to-end proof of ordered client history.
- **Verification:** focused `ClientHistory` and synchronization tests, the stock
  sustained-I/O fixture, and the complete formatting, Clippy, and test suite.
- **Rejected changes:** do not wrap the terminal-state map, merge it into SSP,
  change plan/commit, or replace screen clones without allocation evidence. Do
  not redesign prediction merely to avoid the bounded input clone; that belongs
  to later measured allocation work.
