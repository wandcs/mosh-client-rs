use std::collections::VecDeque;

use crate::limits::{
    ADAPTIVE_PREDICTION_DISABLE_FRAME_MS, ADAPTIVE_PREDICTION_ENABLE_FRAME_MS,
    ADAPTIVE_PREDICTION_GLITCH_MS, MAX_PENDING_PREDICTION_BYTES, MAX_PENDING_PREDICTION_SCALARS,
    MAX_PREDICTION_AGE_MS,
};
use crate::terminal::{TerminalError, TerminalState};

/// Controls when confirmed local terminal predictions are displayed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PredictionMode {
    /// Display predictions on slower links or during a temporary network glitch.
    #[default]
    Adaptive,
    /// Display every eligible prediction after its epoch has been confirmed.
    Always,
    /// Display only authenticated terminal state from the server.
    Never,
}

#[derive(Clone, Debug)]
struct PendingPrediction {
    client_state: u64,
    created_at_ms: u64,
    byte: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EpochConfidence {
    Tentative,
    Confirmed,
}

#[derive(Debug)]
pub(crate) struct LocalPrediction {
    mode: PredictionMode,
    confidence: EpochConfidence,
    adaptive_slow_link: bool,
    adaptive_glitch: bool,
    pending: VecDeque<PendingPrediction>,
    base: Option<TerminalState>,
    projected: Option<TerminalState>,
}

impl LocalPrediction {
    pub(crate) const fn new(mode: PredictionMode) -> Self {
        Self {
            mode,
            confidence: EpochConfidence::Tentative,
            adaptive_slow_link: false,
            adaptive_glitch: false,
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
        if self.mode == PredictionMode::Never {
            return Ok(false);
        }
        let Some(byte) = predictable_byte(bytes) else {
            return Ok(self.clear());
        };

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
        Ok(self.displaying_prediction())
    }

    pub(crate) fn observe_authoritative(
        &mut self,
        authoritative: &TerminalState,
    ) -> Result<(), TerminalError> {
        if self.mode == PredictionMode::Never || self.pending.is_empty() {
            return Ok(());
        }

        let Some(matched_prefix) = self.longest_matching_prefix(authoritative)? else {
            self.clear();
            return Ok(());
        };
        let acknowledged_prefix = authoritative.echo_acknowledgement().map_or(0, |number| {
            self.pending
                .iter()
                .take_while(|prediction| prediction.client_state <= number)
                .count()
        });
        if acknowledged_prefix > matched_prefix {
            self.clear();
            return Ok(());
        }
        if acknowledged_prefix == 0 {
            return Ok(());
        }

        self.confidence = EpochConfidence::Confirmed;
        self.pending.drain(..matched_prefix);
        if self.pending.is_empty() {
            self.base = None;
            self.projected = None;
            self.adaptive_glitch = false;
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
        if self.displaying_prediction() {
            self.projected.as_ref().unwrap_or(authoritative)
        } else {
            authoritative
        }
    }

    pub(crate) fn next_deadline_ms(&self) -> Option<u64> {
        let expiry = self.expiration_deadline_ms();
        let glitch = if self.mode == PredictionMode::Adaptive
            && self.confidence == EpochConfidence::Confirmed
            && !self.adaptive_slow_link
            && !self.adaptive_glitch
        {
            self.pending.front().map(|prediction| {
                prediction
                    .created_at_ms
                    .saturating_add(ADAPTIVE_PREDICTION_GLITCH_MS)
            })
        } else {
            None
        };
        match (expiry, glitch) {
            (Some(expiry), Some(glitch)) => Some(expiry.min(glitch)),
            (Some(deadline), None) | (None, Some(deadline)) => Some(deadline),
            (None, None) => None,
        }
    }

    fn expiration_deadline_ms(&self) -> Option<u64> {
        if self.mode == PredictionMode::Never {
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
            .expiration_deadline_ms()
            .is_some_and(|deadline| now_ms >= deadline)
        {
            self.clear()
        } else {
            false
        }
    }

    pub(crate) fn update_policy(&mut self, now_ms: u64, frame_interval_ms: Option<u64>) -> bool {
        let was_displayed = self.displaying_prediction();
        if self.mode != PredictionMode::Adaptive {
            return false;
        }

        if let Some(frame_interval_ms) = frame_interval_ms {
            if frame_interval_ms > ADAPTIVE_PREDICTION_ENABLE_FRAME_MS {
                self.adaptive_slow_link = true;
            } else if frame_interval_ms <= ADAPTIVE_PREDICTION_DISABLE_FRAME_MS && !was_displayed {
                self.adaptive_slow_link = false;
            }
        }
        if self.pending.is_empty() {
            self.adaptive_glitch = false;
        } else if self.confidence == EpochConfidence::Confirmed
            && self.pending.front().is_some_and(|prediction| {
                now_ms
                    >= prediction
                        .created_at_ms
                        .saturating_add(ADAPTIVE_PREDICTION_GLITCH_MS)
            })
        {
            self.adaptive_glitch = true;
        }
        was_displayed != self.displaying_prediction()
    }

    fn displaying_prediction(&self) -> bool {
        self.confidence == EpochConfidence::Confirmed
            && !self.pending.is_empty()
            && match self.mode {
                PredictionMode::Adaptive => self.adaptive_slow_link || self.adaptive_glitch,
                PredictionMode::Always => true,
                PredictionMode::Never => false,
            }
    }

    fn clear(&mut self) -> bool {
        let displayed_prediction = self.displaying_prediction();
        self.confidence = EpochConfidence::Tentative;
        self.adaptive_glitch = false;
        self.pending.clear();
        self.base = None;
        self.projected = None;
        displayed_prediction
    }

    pub(crate) fn reset(&mut self) -> bool {
        self.clear()
    }

    fn longest_matching_prefix(
        &self,
        authoritative: &TerminalState,
    ) -> Result<Option<usize>, TerminalError> {
        let mut expected = self
            .base
            .as_ref()
            .expect("pending prediction owns a base")
            .clone();
        let mut matched_prefix = expected
            .display_equivalent(authoritative)
            .then_some(0_usize);
        for (index, prediction) in self.pending.iter().enumerate() {
            expected = expected.with_predicted_ascii(prediction.byte)?;
            if expected.display_equivalent(authoritative) {
                matched_prefix = Some(index + 1);
            }
        }
        Ok(matched_prefix)
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
