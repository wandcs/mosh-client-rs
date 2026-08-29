use std::collections::VecDeque;

use crate::limits::{
    MAX_PENDING_PREDICTION_BYTES, MAX_PENDING_PREDICTION_SCALARS, MAX_PREDICTION_AGE_MS,
};
use crate::terminal::{TerminalError, TerminalState};

#[derive(Clone, Debug)]
struct PendingPrediction {
    client_state: u64,
    created_at_ms: u64,
    byte: u8,
}

#[derive(Debug)]
pub(crate) struct LocalPrediction {
    enabled: bool,
    active_epoch: bool,
    pending: VecDeque<PendingPrediction>,
    base: Option<TerminalState>,
    projected: Option<TerminalState>,
}

impl LocalPrediction {
    pub(crate) const fn new() -> Self {
        Self {
            enabled: true,
            active_epoch: false,
            pending: VecDeque::new(),
            base: None,
            projected: None,
        }
    }

    pub(crate) fn observe_input(
        &mut self,
        client_state: u64,
        bytes: &[u8],
        authoritative: &TerminalState,
        now_ms: u64,
    ) -> Result<bool, TerminalError> {
        if !self.enabled {
            return Ok(false);
        }
        let Some(byte) = predictable_byte(bytes) else {
            return Ok(self.clear());
        };

        if !self.active_epoch && !self.pending.is_empty() {
            return Ok(false);
        }
        if self.pending.len() == MAX_PENDING_PREDICTION_SCALARS
            || self.pending.len() == MAX_PENDING_PREDICTION_BYTES
        {
            return Ok(self.clear());
        }

        if self.pending.is_empty() {
            self.base = Some(authoritative.clone());
            self.projected = Some(authoritative.with_predicted_ascii(byte)?);
        } else {
            self.projected = Some(
                self.projected
                    .as_ref()
                    .expect("pending prediction owns a projection")
                    .with_predicted_ascii(byte)?,
            );
        }
        self.pending.push_back(PendingPrediction {
            client_state,
            created_at_ms: now_ms,
            byte,
        });
        Ok(self.active_epoch)
    }

    pub(crate) fn observe_authoritative(
        &mut self,
        echo_acknowledgement: Option<u64>,
        authoritative: &TerminalState,
    ) -> Result<(), TerminalError> {
        if !self.enabled {
            return Ok(());
        }
        let Some(echo_acknowledgement) = echo_acknowledgement else {
            self.clear();
            return Ok(());
        };
        let Some(confirmed_index) = self
            .pending
            .iter()
            .rposition(|prediction| prediction.client_state <= echo_acknowledgement)
        else {
            return Ok(());
        };

        let mut expected = self
            .base
            .as_ref()
            .expect("pending prediction owns a base")
            .clone();
        for prediction in self.pending.iter().take(confirmed_index + 1) {
            expected = expected.with_predicted_ascii(prediction.byte)?;
        }
        if !authoritative.display_equivalent(&expected) {
            self.clear();
            return Ok(());
        }

        self.active_epoch = true;
        self.pending.drain(..=confirmed_index);
        if self.pending.is_empty() {
            self.base = None;
            self.projected = None;
        } else {
            let mut projected = authoritative.clone();
            for prediction in &self.pending {
                projected = projected.with_predicted_ascii(prediction.byte)?;
            }
            self.base = Some(authoritative.clone());
            self.projected = Some(projected);
        }
        Ok(())
    }

    pub(crate) fn display<'a>(&'a self, authoritative: &'a TerminalState) -> &'a TerminalState {
        if self.active_epoch {
            self.projected.as_ref().unwrap_or(authoritative)
        } else {
            authoritative
        }
    }

    pub(crate) fn next_deadline_ms(&self) -> Option<u64> {
        if !self.enabled {
            return None;
        }
        self.pending.front().map(|prediction| {
            prediction
                .created_at_ms
                .saturating_add(MAX_PREDICTION_AGE_MS)
        })
    }

    pub(crate) fn expire(&mut self, now_ms: u64) -> bool {
        if self
            .next_deadline_ms()
            .is_some_and(|deadline| now_ms >= deadline)
        {
            self.clear()
        } else {
            false
        }
    }

    fn clear(&mut self) -> bool {
        let displayed_prediction = self.active_epoch && !self.pending.is_empty();
        self.active_epoch = false;
        self.pending.clear();
        self.base = None;
        self.projected = None;
        displayed_prediction
    }

    pub(crate) fn reset(&mut self) -> bool {
        self.clear()
    }

    #[cfg(test)]
    pub(crate) fn disable(&mut self) {
        self.enabled = false;
        self.active_epoch = false;
        self.pending.clear();
        self.base = None;
        self.projected = None;
    }
}

fn predictable_byte(bytes: &[u8]) -> Option<u8> {
    let [byte] = bytes else {
        return None;
    };
    (byte.is_ascii_graphic() || *byte == b' ').then_some(*byte)
}

#[cfg(test)]
mod tests;
