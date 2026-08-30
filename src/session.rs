mod client_history;
mod reachability;

pub use self::reachability::{SessionInterruption, SessionReachability, SessionReachabilityWatch};

use core::future::Future as _;
use core::task::{Poll, ready};
use std::collections::BTreeMap;
use std::fmt;
use std::future::poll_fn;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Mutex;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, OwnedPermit};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::Bootstrap;
use crate::fragment::{self, FragmentReassembler, ReassemblyError, ReassemblyOutcome};
use crate::instruction::{
    ClientOperation, InstructionError, PROTOCOL_VERSION, SHUTDOWN_STATE, TransportInstruction,
};
use crate::limits::{
    GRACEFUL_CLOSE_ACK_TIMEOUT_MS, MAX_DATAGRAM_BYTES, MAX_FRAGMENT_BODY_BYTES,
    MAX_INPUT_COMMAND_BYTES, PENDING_OUTPUT_CHUNKS, SESSION_COMMAND_QUEUE_CAPACITY,
};
use crate::packet::{Direction, PacketCodec, PacketError, PacketReceiver, SendSequence};
use crate::prediction::{LocalPrediction, PredictionMode};
use crate::synchronization::{
    AcknowledgementDisposition, RemoteStateDisposition, SynchronizationError, SynchronizationState,
};
use crate::terminal::{
    PaintError, TerminalDifference, TerminalError, TerminalPainter, TerminalState, is_valid_size,
};
use crate::timing::{DatagramTiming, SchedulerPoll, SendScheduler, TimingError, WakePlan};

use self::client_history::ClientHistory;
use self::reachability::{ReachabilityError, ReachabilityTracker};

const INITIAL_CHAFF: &[u8] = &[0];

/// The externally visible lifecycle of a Session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SessionState {
    /// The local UDP endpoint exists, but no authenticated remote state has arrived.
    Connecting,
    /// At least one authenticated remote terminal state has been accepted.
    Active,
    /// The Session task has stopped or was dropped.
    Closed,
}

/// The expected reason a Session task stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SessionExit {
    /// A locally requested graceful close completed or reached its bounded ACK wait.
    LocalClosed,
    /// The authenticated peer requested a graceful close.
    RemoteClosed,
    /// The owning application explicitly cancelled the Session.
    Cancelled,
    /// The Session handle or its output consumer was dropped.
    OwnerDropped,
}

/// A failure reported by the Session task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SessionError {
    /// Local socket setup or datagram I/O failed.
    Io(ErrorKind),
    /// The requested initial terminal size is outside the documented limits.
    InvalidTerminalSize,
    /// No complete authenticated remote state arrived before the attachment deadline.
    ConnectionTimeout,
    /// Authenticated input violated the supported protocol or terminal contract.
    Protocol,
    /// A bounded protocol or output resource limit was reached.
    ResourceLimit,
    /// A monotonic protocol identifier or clock value was exhausted.
    StateExhausted,
    /// An internal state invariant could not be satisfied.
    InternalState,
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(formatter, "Session I/O failed: {kind:?}"),
            Self::InvalidTerminalSize => formatter.write_str("invalid terminal size"),
            Self::ConnectionTimeout => formatter.write_str("Session attachment timed out"),
            Self::Protocol => formatter.write_str("remote protocol state is invalid"),
            Self::ResourceLimit => formatter.write_str("Session resource limit reached"),
            Self::StateExhausted => formatter.write_str("Session state space exhausted"),
            Self::InternalState => formatter.write_str("Session internal state is unavailable"),
        }
    }
}

impl std::error::Error for SessionError {}

/// A command rejected before it entered the Session task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SessionCommandError {
    /// The Session is cancelled or closed.
    Closed,
    /// One input command exceeded the documented byte limit.
    InputTooLarge,
    /// The requested terminal size is outside the documented limits.
    InvalidTerminalSize,
}

impl fmt::Display for SessionCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("Session is closed"),
            Self::InputTooLarge => formatter.write_str("input command is too large"),
            Self::InvalidTerminalSize => formatter.write_str("invalid terminal size"),
        }
    }
}

impl std::error::Error for SessionCommandError {}

/// The application-owned control and output side of one Mosh Session.
///
/// The matching [`SessionTask`] must be polled on the application's executor.
pub struct Session {
    commands: mpsc::Sender<SessionCommand>,
    output: mpsc::Receiver<Vec<u8>>,
    cancellation: watch::Sender<bool>,
    graceful_close: watch::Sender<bool>,
    close_requested: Mutex<bool>,
    state: watch::Receiver<SessionState>,
    reachability: watch::Receiver<SessionReachability>,
}

impl Session {
    /// Creates a local Session and its independently driven protocol task.
    ///
    /// Local prediction uses the standard [`PredictionMode::Adaptive`] policy.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::InvalidTerminalSize`] for an unsupported initial
    /// size, or [`SessionError::Io`] when the local UDP endpoint cannot be
    /// created.
    pub async fn connect(
        bootstrap: Bootstrap,
        columns: u32,
        rows: u32,
    ) -> Result<(Self, SessionTask), SessionError> {
        Self::connect_with_prediction_mode(bootstrap, columns, rows, PredictionMode::default())
            .await
    }

    /// Creates a local Session with an explicit local-prediction display policy.
    ///
    /// Prediction never changes authenticated terminal authority. [`PredictionMode::Always`]
    /// displays only eligible predictions from a confirmed epoch, while
    /// [`PredictionMode::Never`] displays only authenticated server state.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::InvalidTerminalSize`] for an unsupported initial
    /// size, or [`SessionError::Io`] when the local UDP endpoint cannot be
    /// created.
    pub async fn connect_with_prediction_mode(
        bootstrap: Bootstrap,
        columns: u32,
        rows: u32,
        prediction_mode: PredictionMode,
    ) -> Result<(Self, SessionTask), SessionError> {
        let (driver, channels) = SessionDriver::connect(bootstrap, columns, rows, prediction_mode)
            .await
            .map_err(SessionError::from)?;
        let SessionChannels {
            commands,
            output,
            cancellation,
            graceful_close,
            state,
            reachability,
        } = channels;
        Ok((
            Self {
                commands,
                output,
                cancellation,
                graceful_close,
                close_requested: Mutex::new(false),
                state,
                reachability,
            },
            SessionTask { driver },
        ))
    }

    /// Queues ordered terminal input. Empty input is accepted as a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`SessionCommandError::InputTooLarge`] when `bytes` exceeds the
    /// documented per-command limit, or [`SessionCommandError::Closed`] after
    /// cancellation or task shutdown.
    pub async fn send_input(&self, bytes: Vec<u8>) -> Result<(), SessionCommandError> {
        if bytes.len() > MAX_INPUT_COMMAND_BYTES {
            return Err(SessionCommandError::InputTooLarge);
        }
        self.send_command(SessionCommand::Input(bytes)).await
    }

    /// Queues an ordered remote terminal resize.
    ///
    /// # Errors
    ///
    /// Returns [`SessionCommandError::InvalidTerminalSize`] for dimensions
    /// outside the documented limits, or [`SessionCommandError::Closed`] after
    /// cancellation or task shutdown.
    pub async fn resize(&self, columns: u32, rows: u32) -> Result<(), SessionCommandError> {
        if !is_valid_size(columns, rows) {
            return Err(SessionCommandError::InvalidTerminalSize);
        }
        self.send_command(SessionCommand::Resize { columns, rows })
            .await
    }

    /// Queues a self-contained full repaint after earlier accepted output.
    ///
    /// # Errors
    ///
    /// Returns [`SessionCommandError::Closed`] after cancellation or task
    /// shutdown.
    pub async fn request_repaint(&self) -> Result<(), SessionCommandError> {
        self.send_command(SessionCommand::Repaint).await
    }

    /// Requests an idempotent authenticated close exchange with the peer.
    ///
    /// Commands accepted before this call remain ordered before the close.
    /// Later input, resize, and repaint requests return
    /// [`SessionCommandError::Closed`]. Use [`Session::cancel`] when the local
    /// task must stop promptly without waiting for the peer.
    pub fn close(&self) {
        let mut requested = self
            .close_requested
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !*requested {
            *requested = true;
            self.graceful_close.send_replace(true);
        }
    }

    /// Requests prompt, idempotent cancellation outside the bounded command queue.
    pub fn cancel(&self) {
        self.cancellation.send_replace(true);
    }

    /// Returns the latest lifecycle state without waiting.
    #[must_use]
    pub fn state(&self) -> SessionState {
        if self.state.has_changed().is_err() {
            SessionState::Closed
        } else {
            *self.state.borrow()
        }
    }

    /// Waits for and returns the next lifecycle state.
    pub async fn state_changed(&mut self) -> SessionState {
        if self.state.changed().await.is_err() {
            SessionState::Closed
        } else {
            *self.state.borrow_and_update()
        }
    }

    /// Returns the latest reachability observation without waiting.
    #[must_use]
    pub fn reachability(&self) -> SessionReachability {
        *self.reachability.borrow()
    }

    /// Creates an independently owned latest-value reachability observer.
    #[must_use]
    pub fn subscribe_reachability(&self) -> SessionReachabilityWatch {
        SessionReachabilityWatch::new(self.reachability.clone())
    }

    /// Receives the next bounded, ordered VT output chunk.
    pub async fn next_output(&mut self) -> Option<Vec<u8>> {
        self.output.recv().await
    }

    async fn send_command(&self, command: SessionCommand) -> Result<(), SessionCommandError> {
        let permit = self
            .commands
            .reserve()
            .await
            .map_err(|_| SessionCommandError::Closed)?;
        let close_requested = self
            .close_requested
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *close_requested || *self.cancellation.borrow() || self.state() == SessionState::Closed {
            return Err(SessionCommandError::Closed);
        }
        permit.send(command);
        Ok(())
    }
}

/// The Session-owned socket, timers, protocol state, and terminal state.
#[must_use = "a SessionTask must be polled for the Session to make progress"]
pub struct SessionTask {
    driver: SessionDriver,
}

impl SessionTask {
    /// Runs the Session until cancellation, owner shutdown, or failure.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::ConnectionTimeout`] when no complete
    /// authenticated remote state arrives within the attachment deadline, or
    /// another stable category for I/O, protocol, resource, or state failure.
    pub async fn run(self) -> Result<SessionExit, SessionError> {
        let state = self.driver.state_tx.clone();
        let result = self.driver.run().await;
        state.send_replace(SessionState::Closed);
        result.map_err(SessionError::from)
    }
}

#[derive(Debug)]
pub(crate) enum SessionCommand {
    Input(Vec<u8>),
    Resize { columns: u32, rows: u32 },
    Repaint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DriverError {
    Io(ErrorKind),
    Packet(PacketError),
    Fragment(fragment::FragmentError),
    Reassembly(ReassemblyError),
    Instruction(InstructionError),
    Synchronization(SynchronizationError),
    Timing(TimingError),
    Reachability(ReachabilityError),
    Terminal(TerminalError),
    Paint(PaintError),
    MissingClientState,
    MissingTerminalState,
    OperationIndexExhausted,
    FragmentIdentifierExhausted,
    ClockExhausted,
    ConnectionTimeout,
    IncompleteDatagramSend,
}

macro_rules! from_error {
    ($source:ty, $variant:ident) => {
        impl From<$source> for DriverError {
            fn from(error: $source) -> Self {
                Self::$variant(error)
            }
        }
    };
}

from_error!(PacketError, Packet);
from_error!(fragment::FragmentError, Fragment);
from_error!(ReassemblyError, Reassembly);
from_error!(InstructionError, Instruction);
from_error!(SynchronizationError, Synchronization);
from_error!(TimingError, Timing);
from_error!(ReachabilityError, Reachability);
from_error!(TerminalError, Terminal);
from_error!(PaintError, Paint);

impl From<std::io::Error> for DriverError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.kind())
    }
}

impl From<DriverError> for SessionError {
    fn from(error: DriverError) -> Self {
        match error {
            DriverError::Io(kind) => Self::Io(kind),
            DriverError::ConnectionTimeout => Self::ConnectionTimeout,
            DriverError::Terminal(TerminalError::InvalidTerminalSize) => Self::InvalidTerminalSize,
            DriverError::Instruction(
                InstructionError::DecodedTooLarge
                | InstructionError::CompressedTooLarge
                | InstructionError::InputTooLarge
                | InstructionError::TooManyClientOperations
                | InstructionError::StateDifferenceTooLarge,
            )
            | DriverError::Paint(_)
            | DriverError::Reassembly(
                ReassemblyError::InstructionTooLarge
                | ReassemblyError::TooManyIncompleteInstructions,
            ) => Self::ResourceLimit,
            DriverError::IncompleteDatagramSend => Self::Io(ErrorKind::WriteZero),
            DriverError::MissingClientState | DriverError::MissingTerminalState => {
                Self::InternalState
            }
            DriverError::OperationIndexExhausted
            | DriverError::FragmentIdentifierExhausted
            | DriverError::ClockExhausted
            | DriverError::Reachability(
                ReachabilityError::TimeMovedBackwards | ReachabilityError::TimerExhausted,
            )
            | DriverError::Packet(PacketError::SequenceExhausted)
            | DriverError::Reassembly(ReassemblyError::TimerExhausted)
            | DriverError::Synchronization(
                SynchronizationError::StateNumberExhausted
                | SynchronizationError::GenerationExhausted,
            )
            | DriverError::Timing(TimingError::TimerExhausted | TimingError::GenerationExhausted) => {
                Self::StateExhausted
            }
            DriverError::Packet(_)
            | DriverError::Fragment(_)
            | DriverError::Reassembly(_)
            | DriverError::Instruction(_)
            | DriverError::Synchronization(_)
            | DriverError::Timing(_)
            | DriverError::Terminal(_) => Self::Protocol,
        }
    }
}

pub(crate) struct SessionChannels {
    pub(crate) commands: mpsc::Sender<SessionCommand>,
    pub(crate) output: mpsc::Receiver<Vec<u8>>,
    pub(crate) cancellation: watch::Sender<bool>,
    pub(crate) graceful_close: watch::Sender<bool>,
    pub(crate) state: watch::Receiver<SessionState>,
    pub(crate) reachability: watch::Receiver<SessionReachability>,
}

pub(crate) struct SessionDriver {
    socket: UdpSocket,
    server_addr: SocketAddrV4,
    packet_codec: PacketCodec,
    packet_receiver: PacketReceiver,
    send_sequence: SendSequence,
    fragment_identifier: InstructionIdentifier,
    reassembler: FragmentReassembler,
    synchronization: SynchronizationState,
    scheduler: SendScheduler,
    datagram_timing: DatagramTiming,
    client_history: ClientHistory,
    terminal_states: BTreeMap<u64, TerminalState>,
    prediction: LocalPrediction,
    painted_state: Option<TerminalState>,
    output_requested: bool,
    force_full_repaint: bool,
    started_at: Instant,
    command_rx: mpsc::Receiver<SessionCommand>,
    output_tx: mpsc::Sender<Vec<u8>>,
    cancellation_rx: watch::Receiver<bool>,
    graceful_close_rx: watch::Receiver<bool>,
    state_tx: watch::Sender<SessionState>,
    reachability: ReachabilityTracker,
    reachability_tx: watch::Sender<SessionReachability>,
    final_terminal: Option<TerminalState>,
}

impl SessionDriver {
    pub(crate) async fn connect(
        bootstrap: Bootstrap,
        columns: u32,
        rows: u32,
        prediction_mode: PredictionMode,
    ) -> Result<(Self, SessionChannels), DriverError> {
        let initial_terminal = TerminalState::new(columns, rows)?;
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?;
        let (server_addr, key) = bootstrap.into_session_parts();
        let packet_codec = PacketCodec::new(&key);
        let packet_receiver = PacketReceiver::new(&key, Direction::ServerToClient);
        drop(key);

        let mut synchronization = SynchronizationState::new();
        let mut client_history = ClientHistory::new();
        client_history.append(ClientOperation::Resize { columns, rows })?;
        let initial_state = synchronization.advance_local()?;
        client_history.checkpoint(initial_state);

        let mut scheduler = SendScheduler::new(0);
        scheduler.note_local_change(0)?;
        let mut terminal_states = BTreeMap::new();
        terminal_states.insert(0, initial_terminal);
        let (command_tx, command_rx) = mpsc::channel(SESSION_COMMAND_QUEUE_CAPACITY);
        let (output_tx, output_rx) = mpsc::channel(PENDING_OUTPUT_CHUNKS);
        let (cancellation_tx, cancellation_rx) = watch::channel(false);
        let (graceful_close_tx, graceful_close_rx) = watch::channel(false);
        let (state_tx, state_rx) = watch::channel(SessionState::Connecting);
        let reachability = ReachabilityTracker::new();
        let (reachability_tx, reachability_rx) = watch::channel(reachability.current());

        Ok((
            Self {
                socket,
                server_addr,
                packet_codec,
                packet_receiver,
                send_sequence: SendSequence::new(),
                fragment_identifier: InstructionIdentifier::new(),
                reassembler: FragmentReassembler::new(0),
                synchronization,
                scheduler,
                datagram_timing: DatagramTiming::new(0),
                client_history,
                terminal_states,
                prediction: LocalPrediction::new(prediction_mode),
                painted_state: None,
                output_requested: false,
                force_full_repaint: false,
                started_at: Instant::now(),
                command_rx,
                output_tx,
                cancellation_rx,
                graceful_close_rx,
                state_tx,
                reachability,
                reachability_tx,
                final_terminal: None,
            },
            SessionChannels {
                commands: command_tx,
                output: output_rx,
                cancellation: cancellation_tx,
                graceful_close: graceful_close_tx,
                state: state_rx,
                reachability: reachability_rx,
            },
        ))
    }

    pub(crate) async fn run(mut self) -> Result<SessionExit, DriverError> {
        let mut inbound = [0_u8; MAX_DATAGRAM_BYTES];

        loop {
            if *self.cancellation_rx.borrow() {
                return Ok(SessionExit::Cancelled);
            }
            let now_ms = self.now_ms()?;
            self.publish_reachability(now_ms)?;
            if self.reachability.attachment_timed_out(now_ms) {
                return Err(DriverError::ConnectionTimeout);
            }
            self.reassembler.expire(now_ms)?;
            if self.prediction.expire(now_ms) {
                self.output_requested = true;
            }
            let estimator = self.datagram_timing.estimator();
            let frame_interval_ms = estimator
                .smoothed_rtt_ms()
                .map(|_| estimator.frame_interval_ms());
            if self.prediction.update_policy(now_ms, frame_interval_ms) {
                self.output_requested = true;
            }
            match self.scheduler.poll(
                now_ms,
                self.datagram_timing.estimator(),
                self.synchronization.latest_unacknowledged_sent_at_ms(),
            )? {
                SchedulerPoll::Send(plan) => {
                    self.send_instruction(plan, now_ms).await?;
                    if *self.cancellation_rx.borrow() {
                        return Ok(SessionExit::Cancelled);
                    }
                }
                SchedulerPoll::Pending { wake_at_ms } => {
                    let wake_at_ms = self
                        .prediction
                        .next_deadline_ms()
                        .map_or(wake_at_ms, |prediction| wake_at_ms.min(prediction));
                    let wake_at_ms = self
                        .reachability
                        .next_deadline_ms()?
                        .map_or(wake_at_ms, |reachability| wake_at_ms.min(reachability));
                    let wake_at = self
                        .started_at
                        .checked_add(Duration::from_millis(wake_at_ms))
                        .ok_or(DriverError::ClockExhausted)?;
                    let needs_output = self.output_requested;
                    match wait_next(
                        &self.socket,
                        WaitChannels {
                            commands: &mut self.command_rx,
                            output: &self.output_tx,
                            cancellation: &mut self.cancellation_rx,
                            graceful_close: Some(&mut self.graceful_close_rx),
                        },
                        &mut inbound,
                        wake_at,
                        needs_output,
                    )
                    .await
                    {
                        Wake::Command(command) => self.handle_command(command)?,
                        Wake::Datagram(result) => {
                            let (length, source) = result?;
                            if self.handle_datagram(&mut inbound[..length], source)?
                                == DatagramOutcome::RemoteShutdown
                            {
                                let now_ms = self.now_ms()?;
                                if !self.send_shutdown_acknowledgement(now_ms).await? {
                                    return Ok(SessionExit::Cancelled);
                                }
                                return self.finish_graceful_close(SessionExit::RemoteClosed).await;
                            }
                        }
                        Wake::Output(permit) => self.emit_output(permit)?,
                        Wake::OwnerDropped => return Ok(SessionExit::OwnerDropped),
                        Wake::Cancelled => return Ok(SessionExit::Cancelled),
                        Wake::GracefulClose => return self.run_local_shutdown().await,
                        Wake::Timer => {}
                    }
                }
            }
        }
    }

    fn now_ms(&self) -> Result<u64, DriverError> {
        u64::try_from(self.started_at.elapsed().as_millis())
            .map_err(|_| DriverError::ClockExhausted)
    }

    fn handle_command(&mut self, command: SessionCommand) -> Result<(), DriverError> {
        let now_ms = self.now_ms()?;
        match command {
            SessionCommand::Input(bytes) => {
                if bytes.is_empty() {
                    return Ok(());
                }
                let state = self.advance_client(ClientOperation::Input(bytes.clone()), now_ms)?;
                let latest = self.synchronization.remote_latest();
                let authoritative = self
                    .terminal_states
                    .get(&latest)
                    .ok_or(DriverError::MissingTerminalState)?;
                if self
                    .prediction
                    .observe_input(state, &bytes, authoritative, now_ms)?
                {
                    self.output_requested = true;
                }
            }
            SessionCommand::Resize { columns, rows } => {
                self.advance_client(ClientOperation::Resize { columns, rows }, now_ms)?;
                let latest = self.synchronization.remote_latest();
                let authoritative = self
                    .terminal_states
                    .get(&latest)
                    .ok_or(DriverError::MissingTerminalState)?;
                if self.prediction.observe_input(
                    self.synchronization.local_latest(),
                    &[],
                    authoritative,
                    now_ms,
                )? {
                    self.output_requested = true;
                }
            }
            SessionCommand::Repaint => {
                self.prediction.reset();
                self.output_requested = true;
                self.force_full_repaint = true;
            }
        }
        Ok(())
    }

    fn advance_client(
        &mut self,
        operation: ClientOperation,
        now_ms: u64,
    ) -> Result<u64, DriverError> {
        self.client_history.append(operation)?;
        let state = self.synchronization.advance_local()?;
        self.client_history.checkpoint(state);
        self.scheduler.note_local_change(now_ms)?;
        Ok(state)
    }

    async fn send_instruction(
        &mut self,
        wake_plan: WakePlan,
        now_ms: u64,
    ) -> Result<(), DriverError> {
        let send_plan = match self.synchronization.plan_send(
            now_ms,
            self.datagram_timing.estimator().retransmission_timeout_ms(),
        ) {
            Ok(plan) => plan,
            Err(SynchronizationError::NoNewState) => {
                let state = self.synchronization.advance_local()?;
                self.client_history.checkpoint(state);
                self.synchronization.plan_send(
                    now_ms,
                    self.datagram_timing.estimator().retransmission_timeout_ms(),
                )?
            }
            Err(error) => return Err(error.into()),
        };
        let difference = self
            .client_history
            .difference(send_plan.base_state, send_plan.target_state)?;
        let instruction = send_plan.instruction(difference, INITIAL_CHAFF.to_vec());
        if !self
            .send_transport_instruction(&instruction, now_ms)
            .await?
        {
            return Ok(());
        }

        let commit = self.synchronization.commit_send(send_plan)?;
        if let Some(evicted) = commit.capacity_evicted_state {
            self.client_history.remove_checkpoint(evicted);
        }
        self.scheduler.commit_send(wake_plan)?;
        Ok(())
    }

    async fn send_transport_instruction(
        &mut self,
        instruction: &TransportInstruction,
        now_ms: u64,
    ) -> Result<bool, DriverError> {
        let compressed = instruction.encode_zlib()?;
        let timestamps = self.datagram_timing.outgoing(now_ms)?;
        let identifier = self.fragment_identifier.take()?;
        let fragment_count = compressed.len().div_ceil(MAX_FRAGMENT_BODY_BYTES);
        let mut plaintext = [0_u8; MAX_DATAGRAM_BYTES];
        let mut datagram = [0_u8; MAX_DATAGRAM_BYTES];

        for (number, body) in compressed.chunks(MAX_FRAGMENT_BODY_BYTES).enumerate() {
            if *self.cancellation_rx.borrow() {
                return Ok(false);
            }
            let number =
                u16::try_from(number).map_err(|_| DriverError::FragmentIdentifierExhausted)?;
            let plaintext_len = fragment::encode(
                timestamps.timestamp,
                timestamps.timestamp_reply,
                identifier,
                number,
                usize::from(number) + 1 == fragment_count,
                body,
                &mut plaintext,
            )?;
            let sequence = self.send_sequence.take()?;
            let datagram_len = self.packet_codec.seal(
                Direction::ClientToServer,
                sequence,
                &plaintext[..plaintext_len],
                &mut datagram,
            )?;
            let sent = self
                .socket
                .send_to(&datagram[..datagram_len], self.server_addr)
                .await?;
            if sent != datagram_len {
                return Err(DriverError::IncompleteDatagramSend);
            }
        }

        Ok(true)
    }

    fn handle_datagram(
        &mut self,
        datagram: &mut [u8],
        source: SocketAddr,
    ) -> Result<DatagramOutcome, DriverError> {
        let Some(instruction) = self.decode_datagram(datagram, source)? else {
            return Ok(DatagramOutcome::Continue);
        };
        if instruction.is_shutdown() {
            self.handle_remote_shutdown(&instruction)?;
            Ok(DatagramOutcome::RemoteShutdown)
        } else {
            self.handle_normal_instruction(&instruction)?;
            Ok(DatagramOutcome::Continue)
        }
    }

    fn decode_datagram(
        &mut self,
        datagram: &mut [u8],
        source: SocketAddr,
    ) -> Result<Option<TransportInstruction>, DriverError> {
        let now_ms = self.now_ms()?;
        let opened = match self
            .packet_receiver
            .open_from(self.server_addr, source, datagram)
        {
            Ok(opened) => opened,
            Err(
                PacketError::DatagramTooShort
                | PacketError::DatagramTooLarge
                | PacketError::WrongDirection
                | PacketError::AuthenticationFailed
                | PacketError::Replay
                | PacketError::TooOld
                | PacketError::UnexpectedSource,
            ) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let sequence = opened.sequence;
        let decoded = fragment::decode(opened.plaintext)?;
        self.datagram_timing.observe_received(
            sequence,
            decoded.timestamp,
            decoded.timestamp_reply,
            now_ms,
        )?;
        let ReassemblyOutcome::Complete(compressed) = self.reassembler.push(now_ms, decoded)?
        else {
            return Ok(None);
        };
        Ok(Some(TransportInstruction::decode_zlib(&compressed)?))
    }

    fn handle_normal_instruction(
        &mut self,
        instruction: &TransportInstruction,
    ) -> Result<(), DriverError> {
        let now_ms = self.now_ms()?;
        let acknowledged_sent_at_ms = self
            .synchronization
            .sent_state_sent_at_ms(instruction.acknowledged_state);
        let transition = self.synchronization.begin_receive(instruction)?;
        if matches!(
            transition.acknowledgement,
            AcknowledgementDisposition::Advanced { .. }
        ) && let Some(sent_at_ms) = acknowledged_sent_at_ms
        {
            self.reachability.note_reply(sent_at_ms);
        }
        self.client_history
            .acknowledge(self.synchronization.known_receiver_state())?;
        self.scheduler.note_acknowledgement_needed(now_ms)?;

        if let RemoteStateDisposition::Apply(plan) = transition.remote_state {
            let base = self
                .terminal_states
                .get(&plan.base_state)
                .ok_or(DriverError::MissingTerminalState)?;
            let difference = TerminalDifference::decode(plan.difference)?;
            let echo_acknowledgement = difference.latest_echo_acknowledgement();
            let next = base.apply(&difference)?;
            let target_state = plan.target_state;
            let commit = self.synchronization.commit_receive(plan)?;
            self.terminal_states.insert(target_state, next);
            self.discard_terminal_states(
                commit.discard_before_state,
                commit.capacity_evicted_state,
            );
            if target_state == self.synchronization.remote_latest() {
                self.reachability.note_contact(now_ms);
                let authoritative = self
                    .terminal_states
                    .get(&target_state)
                    .ok_or(DriverError::MissingTerminalState)?;
                self.prediction
                    .observe_authoritative(echo_acknowledgement, authoritative)?;
                if *self.state_tx.borrow() == SessionState::Connecting {
                    self.state_tx.send_replace(SessionState::Active);
                }
                self.output_requested = true;
            }
        }
        self.publish_reachability(now_ms)?;
        Ok(())
    }

    fn publish_reachability(&mut self, now_ms: u64) -> Result<(), DriverError> {
        if let Some(reachability) = self.reachability.update(now_ms)? {
            self.reachability_tx.send_replace(reachability);
        }
        Ok(())
    }

    fn handle_remote_shutdown(
        &mut self,
        instruction: &TransportInstruction,
    ) -> Result<(), DriverError> {
        let mut shutdown = instruction.clone();
        if shutdown.acknowledged_state == SHUTDOWN_STATE {
            shutdown.acknowledged_state = self.synchronization.known_receiver_state();
        }
        let transition = self.synchronization.begin_shutdown_receive(&shutdown)?;
        self.client_history
            .acknowledge(self.synchronization.known_receiver_state())?;
        let base = self
            .terminal_states
            .get(&transition.base_state)
            .ok_or(DriverError::MissingTerminalState)?;
        let difference = TerminalDifference::decode(transition.difference)?;
        self.final_terminal = Some(base.apply(&difference)?);
        self.prediction.reset();
        self.output_requested = true;
        Ok(())
    }

    async fn send_shutdown_acknowledgement(&mut self, now_ms: u64) -> Result<bool, DriverError> {
        let state = self.synchronization.advance_local()?;
        self.client_history.checkpoint(state);
        let send_plan = self.synchronization.plan_send(
            now_ms,
            self.datagram_timing.estimator().retransmission_timeout_ms(),
        )?;
        let difference = self
            .client_history
            .difference(send_plan.base_state, send_plan.target_state)?;
        let mut instruction = send_plan.instruction(difference, INITIAL_CHAFF.to_vec());
        instruction.acknowledged_state = SHUTDOWN_STATE;
        if !self
            .send_transport_instruction(&instruction, now_ms)
            .await?
        {
            return Ok(false);
        }
        let commit = self.synchronization.commit_send(send_plan)?;
        if let Some(evicted) = commit.capacity_evicted_state {
            self.client_history.remove_checkpoint(evicted);
        }
        Ok(true)
    }

    async fn run_local_shutdown(&mut self) -> Result<SessionExit, DriverError> {
        let started_ms = self.now_ms()?;
        let deadline_ms = started_ms
            .checked_add(GRACEFUL_CLOSE_ACK_TIMEOUT_MS)
            .ok_or(DriverError::ClockExhausted)?;
        let mut next_send_ms = started_ms;
        let mut inbound = [0_u8; MAX_DATAGRAM_BYTES];

        loop {
            if *self.cancellation_rx.borrow() {
                return Ok(SessionExit::Cancelled);
            }
            let now_ms = self.now_ms()?;
            if now_ms >= deadline_ms {
                return Ok(SessionExit::LocalClosed);
            }
            if now_ms >= next_send_ms {
                if !self.send_shutdown_request(now_ms).await? {
                    return Ok(SessionExit::Cancelled);
                }
                next_send_ms = now_ms
                    .checked_add(self.datagram_timing.estimator().retransmission_timeout_ms())
                    .ok_or(DriverError::ClockExhausted)?;
            }

            let wake_ms = next_send_ms.min(deadline_ms);
            let wake_at = self
                .started_at
                .checked_add(Duration::from_millis(wake_ms))
                .ok_or(DriverError::ClockExhausted)?;
            match wait_next(
                &self.socket,
                WaitChannels {
                    commands: &mut self.command_rx,
                    output: &self.output_tx,
                    cancellation: &mut self.cancellation_rx,
                    graceful_close: None,
                },
                &mut inbound,
                wake_at,
                self.output_requested,
            )
            .await
            {
                Wake::Command(command) => self.handle_command(command)?,
                Wake::Datagram(result) => {
                    let (length, source) = result?;
                    let Some(instruction) = self.decode_datagram(&mut inbound[..length], source)?
                    else {
                        continue;
                    };
                    let acknowledged = instruction.acknowledged_state == SHUTDOWN_STATE;
                    if instruction.is_shutdown() {
                        self.handle_remote_shutdown(&instruction)?;
                        let now_ms = self.now_ms()?;
                        if !self.send_shutdown_acknowledgement(now_ms).await? {
                            return Ok(SessionExit::Cancelled);
                        }
                    } else {
                        let mut ordinary = instruction;
                        if acknowledged {
                            ordinary.acknowledged_state =
                                self.synchronization.known_receiver_state();
                        }
                        self.handle_normal_instruction(&ordinary)?;
                    }
                    if acknowledged || self.final_terminal.is_some() {
                        return self.finish_graceful_close(SessionExit::LocalClosed).await;
                    }
                }
                Wake::Output(permit) => self.emit_output(permit)?,
                Wake::OwnerDropped => return Ok(SessionExit::OwnerDropped),
                Wake::Cancelled => return Ok(SessionExit::Cancelled),
                Wake::GracefulClose | Wake::Timer => {}
            }
        }
    }

    async fn send_shutdown_request(&mut self, now_ms: u64) -> Result<bool, DriverError> {
        let base_state = self.synchronization.known_receiver_state();
        let current_state = self.synchronization.local_latest();
        let difference = self.client_history.difference(base_state, current_state)?;
        let instruction = TransportInstruction {
            protocol_version: PROTOCOL_VERSION,
            base_state,
            new_state: SHUTDOWN_STATE,
            acknowledged_state: self.synchronization.remote_latest(),
            discard_before_state: base_state,
            state_difference: difference,
            chaff: INITIAL_CHAFF.to_vec(),
        };
        self.send_transport_instruction(&instruction, now_ms).await
    }

    async fn finish_graceful_close(
        &mut self,
        exit: SessionExit,
    ) -> Result<SessionExit, DriverError> {
        if !self.output_requested {
            return Ok(exit);
        }
        if *self.cancellation_rx.borrow() {
            return Ok(SessionExit::Cancelled);
        }
        let mut cancellation = Box::pin(self.cancellation_rx.changed());
        let mut output = Box::pin(self.output_tx.clone().reserve_owned());
        let permit = poll_fn(|context| {
            if let Poll::Ready(changed) = cancellation.as_mut().poll(context) {
                return Poll::Ready(Err(if changed.is_ok() {
                    SessionExit::Cancelled
                } else {
                    SessionExit::OwnerDropped
                }));
            }
            if let Poll::Ready(permit) = output.as_mut().poll(context) {
                return Poll::Ready(permit.map_err(|_| SessionExit::OwnerDropped));
            }
            Poll::Pending
        })
        .await;
        drop(cancellation);
        drop(output);
        match permit {
            Ok(permit) => {
                self.emit_output(permit)?;
                Ok(exit)
            }
            Err(exit) => Ok(exit),
        }
    }

    fn discard_terminal_states(&mut self, floor: u64, capacity_evicted: Option<u64>) {
        self.terminal_states.retain(|state, _| *state >= floor);
        if let Some(evicted) = capacity_evicted {
            self.terminal_states.remove(&evicted);
        }
    }

    fn emit_output(&mut self, permit: OwnedPermit<Vec<u8>>) -> Result<(), DriverError> {
        let authoritative = self.latest_terminal_state()?;
        let current = self.prediction.display(authoritative);
        let output = if self.force_full_repaint {
            TerminalPainter::full(current)?
        } else if let Some(previous) = &self.painted_state {
            TerminalPainter::incremental(previous, current)?
        } else {
            TerminalPainter::full(current)?
        };
        let painted = current.clone();
        permit.send(output);
        self.painted_state = Some(painted);
        self.output_requested = false;
        self.force_full_repaint = false;
        Ok(())
    }

    fn latest_terminal_state(&self) -> Result<&TerminalState, DriverError> {
        if let Some(final_terminal) = &self.final_terminal {
            return Ok(final_terminal);
        }
        self.terminal_states
            .get(&self.synchronization.remote_latest())
            .ok_or(DriverError::MissingTerminalState)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DatagramOutcome {
    Continue,
    RemoteShutdown,
}

enum Wake {
    Command(SessionCommand),
    Datagram(Result<(usize, SocketAddr), std::io::Error>),
    Output(OwnedPermit<Vec<u8>>),
    OwnerDropped,
    Cancelled,
    GracefulClose,
    Timer,
}

struct WaitChannels<'a> {
    commands: &'a mut mpsc::Receiver<SessionCommand>,
    output: &'a mpsc::Sender<Vec<u8>>,
    cancellation: &'a mut watch::Receiver<bool>,
    graceful_close: Option<&'a mut watch::Receiver<bool>>,
}

async fn wait_next(
    socket: &UdpSocket,
    channels: WaitChannels<'_>,
    inbound: &mut [u8],
    wake_at: Instant,
    needs_output: bool,
) -> Wake {
    let WaitChannels {
        commands,
        output,
        cancellation,
        graceful_close,
    } = channels;
    let mut command = Box::pin(commands.recv());
    let mut datagram = Box::pin(socket.recv_from(inbound));
    let mut output = needs_output.then(|| Box::pin(output.clone().reserve_owned()));
    let mut cancellation = Box::pin(cancellation.changed());
    let mut graceful_close = graceful_close.map(|close| Box::pin(close.changed()));
    let mut timer = Box::pin(tokio::time::sleep_until(wake_at));

    poll_fn(|context| {
        if let Poll::Ready(Ok(())) = cancellation.as_mut().poll(context) {
            return Poll::Ready(Wake::Cancelled);
        }
        if let Poll::Ready(command) = command.as_mut().poll(context) {
            return Poll::Ready(match command {
                Some(command) => Wake::Command(command),
                None => Wake::OwnerDropped,
            });
        }
        if let Some(graceful_close) = graceful_close.as_mut() {
            if let Poll::Ready(Ok(())) = graceful_close.as_mut().poll(context) {
                return Poll::Ready(Wake::GracefulClose);
            }
        }
        if timer.as_mut().poll(context).is_ready() {
            return Poll::Ready(Wake::Timer);
        }
        if let Some(output) = output.as_mut() {
            if let Poll::Ready(permit) = output.as_mut().poll(context) {
                return Poll::Ready(match permit {
                    Ok(permit) => Wake::Output(permit),
                    Err(_) => Wake::OwnerDropped,
                });
            }
        }
        Poll::Ready(Wake::Datagram(ready!(datagram.as_mut().poll(context))))
    })
    .await
}

#[derive(Debug)]
struct InstructionIdentifier(Option<u64>);

impl InstructionIdentifier {
    const fn new() -> Self {
        Self(Some(0))
    }

    fn take(&mut self) -> Result<u64, DriverError> {
        let identifier = self.0.ok_or(DriverError::FragmentIdentifierExhausted)?;
        self.0 = identifier.checked_add(1);
        Ok(identifier)
    }
}

#[cfg(test)]
mod tests;
