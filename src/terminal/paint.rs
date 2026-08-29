use super::TerminalState;
use crate::limits::MAX_OUTPUT_CHUNK_BYTES;

const INITIAL_PROFILE_POLICY_RESET: &[u8] = b"\x1b[?1001l\x1b[?1004l\x1b[?1015l";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PaintError {
    OutputTooLarge,
}

pub(crate) struct TerminalPainter;

impl TerminalPainter {
    pub(crate) fn full(state: &TerminalState) -> Result<Vec<u8>, PaintError> {
        let formatted = state.screen().state_formatted();
        let mut output = Vec::with_capacity(INITIAL_PROFILE_POLICY_RESET.len() + formatted.len());
        output.extend_from_slice(INITIAL_PROFILE_POLICY_RESET);
        output.extend_from_slice(&formatted);
        bounded(output)
    }

    pub(crate) fn incremental(
        previous: &TerminalState,
        current: &TerminalState,
    ) -> Result<Vec<u8>, PaintError> {
        if previous.screen().size() != current.screen().size() {
            return Self::full(current);
        }
        bounded(current.screen().state_diff(previous.screen()))
    }
}

fn bounded(output: Vec<u8>) -> Result<Vec<u8>, PaintError> {
    if output.len() > MAX_OUTPUT_CHUNK_BYTES {
        return Err(PaintError::OutputTooLarge);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use prost::Message as _;

    use super::*;
    use crate::terminal::{
        TerminalDifference, WireHostBytes, WireHostDifference, WireHostOperation, WireHostSize,
    };

    fn apply(state: &TerminalState, bytes: &[u8]) -> TerminalState {
        let wire = WireHostDifference {
            operations: vec![WireHostOperation {
                host_bytes: Some(WireHostBytes {
                    bytes: bytes.to_vec(),
                }),
                size: None,
                echo_acknowledgement: None,
            }],
        }
        .encode_to_vec();
        state
            .apply(&TerminalDifference::decode(&wire).unwrap())
            .unwrap()
    }

    fn assert_repaints(source: &TerminalState, paint: &[u8]) {
        let (rows, columns) = source.screen().size();
        let mut projection = vt100::Parser::new(rows, columns, 0);
        projection.process(paint);
        assert_eq!(
            projection.screen().state_formatted(),
            source.screen().state_formatted()
        );
    }

    #[test]
    fn full_repaint_is_self_contained_and_has_no_system_effects() {
        let state = apply(
            &TerminalState::new(80, 24).unwrap(),
            b"\x1b[31mA\x1b[0m\x07\x1b]2;secret title\x07B",
        );
        let paint = TerminalPainter::full(&state).unwrap();
        assert_repaints(&state, &paint);
        assert!(!paint.contains(&0x07));
        assert!(!paint.windows(2).any(|bytes| bytes == b"]2"));
        assert!(!paint.windows(2).any(|bytes| bytes == b"]52"));
        assert!(paint.starts_with(INITIAL_PROFILE_POLICY_RESET));
    }

    #[test]
    fn incremental_repaint_transforms_the_previous_projection() {
        let previous = apply(&TerminalState::new(80, 24).unwrap(), b"first");
        let current = apply(&previous, b"\rsecond");
        let mut projection = vt100::Parser::new(24, 80, 0);
        projection.process(&TerminalPainter::full(&previous).unwrap());
        projection.process(&TerminalPainter::incremental(&previous, &current).unwrap());
        assert_eq!(
            projection.screen().state_formatted(),
            current.screen().state_formatted()
        );
    }

    #[test]
    fn size_change_forces_a_self_contained_full_repaint() {
        let previous = apply(&TerminalState::new(80, 24).unwrap(), b"old");
        let resize = WireHostDifference {
            operations: vec![WireHostOperation {
                host_bytes: None,
                size: Some(WireHostSize {
                    columns: 81,
                    rows: 25,
                }),
                echo_acknowledgement: None,
            }],
        }
        .encode_to_vec();
        let current = previous
            .apply(&TerminalDifference::decode(&resize).unwrap())
            .unwrap();
        let incremental = TerminalPainter::incremental(&previous, &current).unwrap();
        assert_eq!(incremental, TerminalPainter::full(&current).unwrap());
        assert_repaints(&current, &incremental);
    }

    #[test]
    fn bounded_output_rejects_oversized_chunks() {
        assert_eq!(
            bounded(vec![0; MAX_OUTPUT_CHUNK_BYTES + 1]),
            Err(PaintError::OutputTooLarge)
        );
    }
}
