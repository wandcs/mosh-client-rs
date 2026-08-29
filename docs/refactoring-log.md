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
