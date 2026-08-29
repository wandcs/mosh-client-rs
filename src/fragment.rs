use core::fmt;
use std::collections::BTreeMap;

use crate::limits::{
    INCOMPLETE_INSTRUCTION_LIFETIME_MS, MAX_COMPRESSED_INSTRUCTION_BYTES, MAX_FRAGMENT_BODY_BYTES,
    MAX_FRAGMENTS_PER_INSTRUCTION, MAX_INCOMPLETE_INSTRUCTIONS,
    MAX_TOTAL_INCOMPLETE_FRAGMENT_BYTES,
};

const TIMESTAMP_BYTES: usize = 2;
const TIMESTAMP_REPLY_BYTES: usize = 2;
const IDENTIFIER_BYTES: usize = 8;
const FRAGMENT_WORD_BYTES: usize = 2;
const HEADER_BYTES: usize =
    TIMESTAMP_BYTES + TIMESTAMP_REPLY_BYTES + IDENTIFIER_BYTES + FRAGMENT_WORD_BYTES;
const FINAL_FRAGMENT_BIT: u16 = 1 << 15;
const MAX_FRAGMENT_NUMBER: u16 = FINAL_FRAGMENT_BIT - 1;
const NO_TIMESTAMP_REPLY: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FragmentError {
    TooShort,
    BodyTooLarge,
    FragmentNumberOutOfRange,
    OutputTooSmall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReassemblyError {
    FragmentNumberOutOfRange,
    ConflictingFragment,
    InstructionTooLarge,
    TooManyIncompleteInstructions,
    TotalStorageExceeded,
    TimeMovedBackwards,
    TimerExhausted,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ReassemblyOutcome {
    Pending,
    Duplicate,
    Complete(Vec<u8>),
}

#[derive(Clone, Copy)]
pub(crate) struct Fragment<'a> {
    pub(crate) timestamp: u16,
    pub(crate) timestamp_reply: Option<u16>,
    pub(crate) identifier: u64,
    pub(crate) number: u16,
    pub(crate) is_final: bool,
    pub(crate) body: &'a [u8],
}

#[derive(Debug)]
struct StoredFragment {
    body: Vec<u8>,
    is_final: bool,
}

#[derive(Debug)]
struct IncompleteInstruction {
    created_at_ms: u64,
    stored_bytes: usize,
    final_number: Option<u16>,
    fragments: BTreeMap<u16, StoredFragment>,
}

impl IncompleteInstruction {
    fn new(created_at_ms: u64) -> Self {
        Self {
            created_at_ms,
            stored_bytes: 0,
            final_number: None,
            fragments: BTreeMap::new(),
        }
    }

    fn classify(&self, fragment: &Fragment<'_>) -> Result<InsertKind, ReassemblyError> {
        if let Some(existing) = self.fragments.get(&fragment.number) {
            return if existing.body == fragment.body && existing.is_final == fragment.is_final {
                Ok(InsertKind::Duplicate)
            } else {
                Err(ReassemblyError::ConflictingFragment)
            };
        }
        if self
            .final_number
            .is_some_and(|final_number| fragment.number > final_number)
            || fragment.is_final
                && (self.final_number.is_some()
                    || self
                        .fragments
                        .last_key_value()
                        .is_some_and(|(number, _)| *number > fragment.number))
        {
            return Err(ReassemblyError::ConflictingFragment);
        }
        Ok(InsertKind::New)
    }

    fn insert(&mut self, fragment: &Fragment<'_>) {
        if fragment.is_final {
            self.final_number = Some(fragment.number);
        }
        self.stored_bytes += fragment.body.len();
        self.fragments.insert(
            fragment.number,
            StoredFragment {
                body: fragment.body.to_vec(),
                is_final: fragment.is_final,
            },
        );
    }

    fn is_complete(&self) -> bool {
        let Some(final_number) = self.final_number else {
            return false;
        };
        self.fragments.len() == usize::from(final_number) + 1
            && self.fragments.keys().copied().eq(0..=final_number)
    }

    fn assemble(self) -> Vec<u8> {
        let mut output = Vec::with_capacity(self.stored_bytes);
        for fragment in self.fragments.into_values() {
            output.extend_from_slice(&fragment.body);
        }
        output
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InsertKind {
    New,
    Duplicate,
}

#[derive(Debug)]
pub(crate) struct FragmentReassembler {
    incomplete: BTreeMap<u64, IncompleteInstruction>,
    stored_bytes: usize,
    last_observed_ms: u64,
}

impl FragmentReassembler {
    pub(crate) const fn new(started_at_ms: u64) -> Self {
        Self {
            incomplete: BTreeMap::new(),
            stored_bytes: 0,
            last_observed_ms: started_at_ms,
        }
    }

    pub(crate) fn push(
        &mut self,
        now_ms: u64,
        fragment: Fragment<'_>,
    ) -> Result<ReassemblyOutcome, ReassemblyError> {
        self.observe_time(now_ms)?;
        self.expire_due(now_ms)?;
        if usize::from(fragment.number) >= MAX_FRAGMENTS_PER_INSTRUCTION {
            return Err(ReassemblyError::FragmentNumberOutOfRange);
        }

        if !self.incomplete.contains_key(&fragment.identifier) {
            if fragment.number == 0 && fragment.is_final {
                return if fragment.body.len() <= MAX_COMPRESSED_INSTRUCTION_BYTES {
                    Ok(ReassemblyOutcome::Complete(fragment.body.to_vec()))
                } else {
                    Err(ReassemblyError::InstructionTooLarge)
                };
            }
            if self.incomplete.len() == MAX_INCOMPLETE_INSTRUCTIONS {
                return Err(ReassemblyError::TooManyIncompleteInstructions);
            }
            if fragment.body.len() > MAX_COMPRESSED_INSTRUCTION_BYTES {
                return Err(ReassemblyError::InstructionTooLarge);
            }
            let total_bytes = self
                .stored_bytes
                .checked_add(fragment.body.len())
                .ok_or(ReassemblyError::TotalStorageExceeded)?;
            if total_bytes > MAX_TOTAL_INCOMPLETE_FRAGMENT_BYTES {
                return Err(ReassemblyError::TotalStorageExceeded);
            }
            let mut instruction = IncompleteInstruction::new(now_ms);
            instruction.insert(&fragment);
            self.incomplete.insert(fragment.identifier, instruction);
            self.stored_bytes = total_bytes;
            return Ok(ReassemblyOutcome::Pending);
        }

        let insert_kind = self
            .incomplete
            .get(&fragment.identifier)
            .expect("the incomplete instruction exists")
            .classify(&fragment);
        let insert_kind = match insert_kind {
            Ok(kind) => kind,
            Err(error) => {
                self.discard(fragment.identifier);
                return Err(error);
            }
        };
        if insert_kind == InsertKind::Duplicate {
            return Ok(ReassemblyOutcome::Duplicate);
        }

        let instruction_bytes = self
            .incomplete
            .get(&fragment.identifier)
            .expect("the incomplete instruction exists")
            .stored_bytes
            .checked_add(fragment.body.len())
            .ok_or(ReassemblyError::InstructionTooLarge)?;
        if instruction_bytes > MAX_COMPRESSED_INSTRUCTION_BYTES {
            return Err(ReassemblyError::InstructionTooLarge);
        }
        let total_bytes = self
            .stored_bytes
            .checked_add(fragment.body.len())
            .ok_or(ReassemblyError::TotalStorageExceeded)?;
        if total_bytes > MAX_TOTAL_INCOMPLETE_FRAGMENT_BYTES {
            return Err(ReassemblyError::TotalStorageExceeded);
        }

        let instruction = self
            .incomplete
            .get_mut(&fragment.identifier)
            .expect("the incomplete instruction exists");
        instruction.insert(&fragment);
        self.stored_bytes = total_bytes;
        if !instruction.is_complete() {
            return Ok(ReassemblyOutcome::Pending);
        }

        let complete = self
            .incomplete
            .remove(&fragment.identifier)
            .expect("the completed instruction exists");
        self.stored_bytes -= complete.stored_bytes;
        Ok(ReassemblyOutcome::Complete(complete.assemble()))
    }

    pub(crate) fn expire(&mut self, now_ms: u64) -> Result<usize, ReassemblyError> {
        self.observe_time(now_ms)?;
        self.expire_due(now_ms)
    }

    pub(crate) fn clear(&mut self) -> usize {
        let discarded = self.incomplete.len();
        self.incomplete.clear();
        self.stored_bytes = 0;
        discarded
    }

    pub(crate) fn incomplete_count(&self) -> usize {
        self.incomplete.len()
    }

    pub(crate) const fn stored_bytes(&self) -> usize {
        self.stored_bytes
    }

    fn observe_time(&mut self, now_ms: u64) -> Result<(), ReassemblyError> {
        if now_ms < self.last_observed_ms {
            return Err(ReassemblyError::TimeMovedBackwards);
        }
        self.last_observed_ms = now_ms;
        Ok(())
    }

    fn expire_due(&mut self, now_ms: u64) -> Result<usize, ReassemblyError> {
        let mut expired = Vec::new();
        for (identifier, instruction) in &self.incomplete {
            let deadline = instruction
                .created_at_ms
                .checked_add(INCOMPLETE_INSTRUCTION_LIFETIME_MS)
                .ok_or(ReassemblyError::TimerExhausted)?;
            if deadline <= now_ms {
                expired.push(*identifier);
            }
        }
        for identifier in &expired {
            self.discard(*identifier);
        }
        Ok(expired.len())
    }

    fn discard(&mut self, identifier: u64) {
        if let Some(instruction) = self.incomplete.remove(&identifier) {
            self.stored_bytes -= instruction.stored_bytes;
        }
    }
}

impl fmt::Debug for Fragment<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Fragment")
            .field("timestamp", &self.timestamp)
            .field("timestamp_reply", &self.timestamp_reply)
            .field("identifier", &self.identifier)
            .field("number", &self.number)
            .field("is_final", &self.is_final)
            .field("body_bytes", &self.body.len())
            .field("body", &"[REDACTED]")
            .finish()
    }
}

pub(crate) fn encode(
    timestamp: u16,
    timestamp_reply: Option<u16>,
    identifier: u64,
    number: u16,
    is_final: bool,
    body: &[u8],
    output: &mut [u8],
) -> Result<usize, FragmentError> {
    if number > MAX_FRAGMENT_NUMBER {
        return Err(FragmentError::FragmentNumberOutOfRange);
    }
    if body.len() > MAX_FRAGMENT_BODY_BYTES {
        return Err(FragmentError::BodyTooLarge);
    }
    let encoded_len = HEADER_BYTES + body.len();
    if output.len() < encoded_len {
        return Err(FragmentError::OutputTooSmall);
    }

    output[..2].copy_from_slice(&timestamp.to_be_bytes());
    output[2..4].copy_from_slice(&timestamp_reply.unwrap_or(NO_TIMESTAMP_REPLY).to_be_bytes());
    output[4..12].copy_from_slice(&identifier.to_be_bytes());
    let fragment_word = number | if is_final { FINAL_FRAGMENT_BIT } else { 0 };
    output[12..14].copy_from_slice(&fragment_word.to_be_bytes());
    output[HEADER_BYTES..encoded_len].copy_from_slice(body);
    Ok(encoded_len)
}

pub(crate) fn decode(plaintext: &[u8]) -> Result<Fragment<'_>, FragmentError> {
    if plaintext.len() < HEADER_BYTES {
        return Err(FragmentError::TooShort);
    }
    let body = &plaintext[HEADER_BYTES..];
    if body.len() > MAX_FRAGMENT_BODY_BYTES {
        return Err(FragmentError::BodyTooLarge);
    }

    let timestamp = u16::from_be_bytes([plaintext[0], plaintext[1]]);
    let raw_reply = u16::from_be_bytes([plaintext[2], plaintext[3]]);
    let mut identifier = [0_u8; IDENTIFIER_BYTES];
    identifier.copy_from_slice(&plaintext[4..12]);
    let fragment_word = u16::from_be_bytes([plaintext[12], plaintext[13]]);

    Ok(Fragment {
        timestamp,
        timestamp_reply: (raw_reply != NO_TIMESTAMP_REPLY).then_some(raw_reply),
        identifier: u64::from_be_bytes(identifier),
        number: fragment_word & MAX_FRAGMENT_NUMBER,
        is_final: fragment_word & FINAL_FRAGMENT_BIT != 0,
        body,
    })
}

#[cfg(test)]
mod tests;
