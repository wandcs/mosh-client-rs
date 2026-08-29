//! Internal entry points for `cargo-fuzz` targets.
//!
//! This module exists only when rustc receives `--cfg fuzzing`; it is absent
//! from normal library builds and from the published API surface.

use std::net::Ipv4Addr;

use crate::Bootstrap;
use crate::crypto::SessionKey;
use crate::fragment::{Fragment, FragmentReassembler};
use crate::instruction::{PROTOCOL_VERSION, TransportInstruction};
use crate::limits::{
    MAX_COMPRESSED_INSTRUCTION_BYTES, MAX_DATAGRAM_BYTES, MAX_RECEIVED_REFERENCE_STATES,
    MAX_SENT_STATES_AFTER_ACK,
};
use crate::packet::{Direction, PacketReceiver};
use crate::synchronization::{RemoteStateDisposition, SynchronizationState};
use crate::terminal::{TerminalDifference, TerminalPainter, TerminalState};
use crate::timing::{RttEstimator, SchedulerPoll, SendScheduler};

/// Exercise every retained untrusted parser without weakening authentication
/// or bypassing production limits.
pub fn exercise_untrusted_parsers(data: &[u8]) {
    let _ = Bootstrap::parse(Ipv4Addr::LOCALHOST, data);
    let _ = TransportInstruction::decode_zlib(data);

    if let Ok(fragment) = crate::fragment::decode(data) {
        let mut reassembler = FragmentReassembler::new(0);
        let _ = reassembler.push(0, fragment);
        assert!(reassembler.incomplete_count() <= 1);
        assert!(reassembler.stored_bytes() <= MAX_COMPRESSED_INSTRUCTION_BYTES);
    }

    if let Ok(difference) = TerminalDifference::decode(data) {
        let initial = TerminalState::new(80, 24).expect("the fixed fuzz size is valid");
        if let Ok(next) = initial.apply(&difference) {
            let _ = TerminalPainter::full(&next);
            let _ = TerminalPainter::incremental(&initial, &next);
        }
    }

    let key = SessionKey::new([0x5a; 16]);
    let mut receiver = PacketReceiver::new(&key, Direction::ServerToClient);
    let mut datagram = data[..data.len().min(MAX_DATAGRAM_BYTES + 1)].to_vec();
    let _ = receiver.open(&mut datagram);
}

/// Interpret bounded input as fragment, SSP, and send-scheduler transitions.
pub fn exercise_state_transitions(data: &[u8]) {
    let mut now_ms = 0_u64;
    let mut reassembler = FragmentReassembler::new(now_ms);
    let mut synchronization = SynchronizationState::new();
    let mut scheduler = SendScheduler::new(now_ms);
    let mut timing = RttEstimator::new();

    for chunk in data.chunks(16).take(256) {
        let operation = chunk.first().copied().unwrap_or_default() % 8;
        now_ms = now_ms.saturating_add(u64::from(chunk.get(1).copied().unwrap_or_default() % 32));
        let number = u64::from_le_bytes(padded_eight(chunk));

        match operation {
            0 => {
                let fragment = Fragment {
                    timestamp: u16::try_from(now_ms & 0xffff).expect("masked timestamp fits"),
                    timestamp_reply: None,
                    identifier: number % 4,
                    number: u16::from(chunk.get(2).copied().unwrap_or_default() % 8),
                    is_final: chunk.get(3).is_some_and(|byte| byte & 1 != 0),
                    body: chunk.get(4..).unwrap_or_default(),
                };
                let _ = reassembler.push(now_ms, fragment);
            }
            1 => {
                let _ = reassembler.expire(now_ms);
            }
            2 => {
                let _ = synchronization.advance_local();
            }
            3 => {
                if let Ok(plan) = synchronization.plan_send(now_ms, 50) {
                    let _ = synchronization.commit_send(plan);
                }
            }
            4 => {
                let base_state = number % 64;
                let instruction = TransportInstruction {
                    protocol_version: if chunk.get(2).is_some_and(|byte| byte & 1 != 0) {
                        PROTOCOL_VERSION
                    } else {
                        PROTOCOL_VERSION + 1
                    },
                    base_state,
                    new_state: u64::from(chunk.get(3).copied().unwrap_or_default()) % 64,
                    acknowledged_state: u64::from(chunk.get(4).copied().unwrap_or_default()) % 64,
                    discard_before_state: u64::from(chunk.get(5).copied().unwrap_or_default()) % 64,
                    state_difference: chunk.get(6..).unwrap_or_default().to_vec(),
                    chaff: Vec::new(),
                };
                if let Ok(transition) = synchronization.begin_receive(&instruction) {
                    if let RemoteStateDisposition::Apply(plan) = transition.remote_state {
                        let _ = synchronization.commit_receive(plan);
                    }
                }
            }
            5 => {
                let _ = scheduler.note_local_change(now_ms);
            }
            6 => {
                let _ = scheduler.note_acknowledgement_needed(now_ms);
            }
            _ => {
                let sample = number % 60_001;
                let _ = timing.observe_sample(sample);
                if let Ok(SchedulerPoll::Send(plan)) = scheduler.poll(now_ms, &timing, None) {
                    let _ = scheduler.commit_send(plan);
                }
            }
        }

        assert!(reassembler.incomplete_count() <= 1);
        assert!(reassembler.stored_bytes() <= MAX_COMPRESSED_INSTRUCTION_BYTES);
        let (sent, received) = synchronization.fuzz_retained_counts();
        assert!(sent <= MAX_SENT_STATES_AFTER_ACK);
        assert!(received <= MAX_RECEIVED_REFERENCE_STATES);
    }
}

fn padded_eight(bytes: &[u8]) -> [u8; 8] {
    let mut padded = [0_u8; 8];
    let copied = bytes.len().min(padded.len());
    padded[..copied].copy_from_slice(&bytes[..copied]);
    padded
}
