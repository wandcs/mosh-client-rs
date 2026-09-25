use core::fmt;

use unicode_width::UnicodeWidthChar as _;

use super::{TerminalDifference, TerminalError, TerminalOperation};
use crate::limits::{
    MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS, MAX_TERMINAL_VISIBLE_CELLS,
    MAX_UNICODE_SCALARS_PER_CELL,
};

const VT100_CELL_CONTENT_BYTES: usize = 22;
const MAX_BYTES_BEFORE_COMBINING_APPEND: usize = VT100_CELL_CONTENT_BYTES - 4;

#[derive(Clone)]
pub(crate) struct TerminalState {
    screen: vt100::Screen,
    echo_acknowledgement: Option<u64>,
}

impl fmt::Debug for TerminalState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (rows, columns) = self.screen.size();
        formatter
            .debug_struct("TerminalState")
            .field("rows", &rows)
            .field("columns", &columns)
            .field("echo_acknowledgement", &self.echo_acknowledgement)
            .field("contents", &"[REDACTED]")
            .finish()
    }
}

impl TerminalState {
    pub(crate) fn new(columns: u32, rows: u32) -> Result<Self, TerminalError> {
        let (rows, columns) = checked_size(columns, rows)?;
        Ok(Self {
            screen: vt100::Parser::new(rows, columns, 0).screen().clone(),
            echo_acknowledgement: None,
        })
    }

    pub(crate) fn apply(&self, difference: &TerminalDifference) -> Result<Self, TerminalError> {
        let mut next = self.clone();
        for operation in &difference.operations {
            match operation {
                TerminalOperation::HostBytes(bytes) => next.apply_host_bytes(bytes)?,
                TerminalOperation::Resize { columns, rows } => {
                    let (rows, columns) = checked_size(*columns, *rows)?;
                    next.screen.set_size(rows, columns);
                }
                TerminalOperation::EchoAcknowledgement(number) => {
                    if next
                        .echo_acknowledgement
                        .is_some_and(|current| *number < current)
                    {
                        return Err(TerminalError::InvalidOperation);
                    }
                    next.echo_acknowledgement = Some(*number);
                }
            }
        }
        Ok(next)
    }

    pub(crate) const fn screen(&self) -> &vt100::Screen {
        &self.screen
    }

    pub(crate) const fn echo_acknowledgement(&self) -> Option<u64> {
        self.echo_acknowledgement
    }

    #[cfg(test)]
    pub(crate) fn with_test_echo_acknowledgement(mut self, number: u64) -> Self {
        self.echo_acknowledgement = Some(number);
        self
    }

    pub(crate) fn with_predicted_ascii(&self, byte: u8) -> Result<Self, TerminalError> {
        debug_assert!(byte.is_ascii_graphic() || byte == b' ');
        let mut predicted = self.clone();
        predicted.apply_host_bytes(&[byte])?;
        Ok(predicted)
    }

    pub(crate) fn display_equivalent(&self, other: &Self) -> bool {
        self.screen.state_formatted() == other.screen.state_formatted()
    }

    fn apply_host_bytes(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        validate_host_bytes(bytes)?;
        let (rows, columns) = self.screen.size();
        let mut parser =
            vt100::Parser::new_with_callbacks(rows, columns, 0, TerminalCallbacks::default());
        *parser.screen_mut() = self.screen.clone();
        parser.process(bytes);
        if let Some(error) = parser.callbacks().unsupported_sequence {
            return Err(error);
        }
        validate_screen(parser.screen())?;
        self.screen = parser.screen().clone();
        Ok(())
    }
}

pub(crate) fn is_valid_size(columns: u32, rows: u32) -> bool {
    checked_size(columns, rows).is_ok()
}

fn checked_size(columns: u32, rows: u32) -> Result<(u16, u16), TerminalError> {
    let cells = usize::try_from(columns)
        .ok()
        .and_then(|columns| usize::try_from(rows).ok().map(|rows| columns * rows));
    if columns == 0
        || columns > MAX_TERMINAL_COLUMNS
        || rows == 0
        || rows > MAX_TERMINAL_ROWS
        || cells.is_none_or(|cells| cells > MAX_TERMINAL_VISIBLE_CELLS)
    {
        return Err(TerminalError::InvalidTerminalSize);
    }
    Ok((
        u16::try_from(rows).map_err(|_| TerminalError::InvalidTerminalSize)?,
        u16::try_from(columns).map_err(|_| TerminalError::InvalidTerminalSize)?,
    ))
}

fn validate_host_bytes(bytes: &[u8]) -> Result<(), TerminalError> {
    core::str::from_utf8(bytes).map_err(|_| TerminalError::InvalidUtf8)?;
    let mut parser = vte::Parser::new();
    let mut budget = ScalarBudget::default();
    parser.advance(&mut budget, bytes);
    let prints_before_sentinel = budget.prints;
    parser.advance(&mut budget, b"X");
    if let Some(error) = budget.error {
        return Err(error);
    }
    if budget.prints != prints_before_sentinel + 1 || budget.last_printed != Some('X') {
        return Err(TerminalError::IncompleteSequence);
    }
    Ok(())
}

fn validate_screen(screen: &vt100::Screen) -> Result<(), TerminalError> {
    let (rows, columns) = screen.size();
    for row in 0..rows {
        for column in 0..columns {
            let Some(cell) = screen.cell(row, column) else {
                continue;
            };
            if cell.contents().chars().count() > MAX_UNICODE_SCALARS_PER_CELL {
                return Err(TerminalError::TooManyScalarsInCell);
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct ScalarBudget {
    scalars: usize,
    utf8_bytes: usize,
    prints: usize,
    last_printed: Option<char>,
    error: Option<TerminalError>,
}

impl ScalarBudget {
    fn reset_cluster(&mut self) {
        self.scalars = 0;
        self.utf8_bytes = 0;
    }
}

impl vte::Perform for ScalarBudget {
    fn print(&mut self, character: char) {
        self.prints += 1;
        self.last_printed = Some(character);
        let Some(width) = character.width() else {
            self.error = Some(TerminalError::UnsupportedControl);
            return;
        };
        if width == 0 {
            if self.scalars == 0 {
                self.error = Some(TerminalError::TooManyScalarsInCell);
                return;
            }
            if self.scalars == MAX_UNICODE_SCALARS_PER_CELL {
                self.error = Some(TerminalError::TooManyScalarsInCell);
                return;
            }
            if self.utf8_bytes >= MAX_BYTES_BEFORE_COMBINING_APPEND {
                self.error = Some(TerminalError::CellEncodingTooLarge);
                return;
            }
            self.scalars += 1;
            self.utf8_bytes += character.len_utf8();
        } else {
            self.scalars = 1;
            self.utf8_bytes = character.len_utf8();
        }
    }

    fn execute(&mut self, _byte: u8) {
        self.reset_cluster();
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        self.reset_cluster();
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        self.reset_cluster();
    }

    fn csi_dispatch(
        &mut self,
        _params: &vte::Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        if action != 'm' {
            self.reset_cluster();
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {
        self.reset_cluster();
    }
}

#[derive(Default)]
struct TerminalCallbacks {
    unsupported_sequence: Option<TerminalError>,
}

impl vt100::Callbacks for TerminalCallbacks {
    fn unhandled_char(&mut self, _screen: &mut vt100::Screen, _character: char) {
        self.unsupported_sequence = Some(TerminalError::UnsupportedCharacter);
    }

    fn unhandled_control(&mut self, _screen: &mut vt100::Screen, _byte: u8) {
        self.unsupported_sequence = Some(TerminalError::UnsupportedControl);
    }

    fn unhandled_escape(
        &mut self,
        _screen: &mut vt100::Screen,
        _intermediate_1: Option<u8>,
        _intermediate_2: Option<u8>,
        _byte: u8,
    ) {
        self.unsupported_sequence = Some(TerminalError::UnsupportedEscape);
    }

    fn unhandled_csi(
        &mut self,
        _screen: &mut vt100::Screen,
        intermediate_1: Option<u8>,
        _intermediate_2: Option<u8>,
        params: &[&[u16]],
        character: char,
    ) {
        if is_suppressed_external_policy_sequence(intermediate_1, params, character) {
            return;
        }
        self.unsupported_sequence = Some(TerminalError::UnsupportedCsi {
            intermediate: intermediate_1,
            parameter: params
                .first()
                .and_then(|parameter| parameter.first())
                .copied(),
            final_character: character,
        });
    }

    fn unhandled_osc(&mut self, _screen: &mut vt100::Screen, _params: &[&[u8]]) {
        self.unsupported_sequence = Some(TerminalError::UnsupportedOsc);
    }
}

fn is_suppressed_external_policy_sequence(
    intermediate: Option<u8>,
    params: &[&[u16]],
    final_character: char,
) -> bool {
    intermediate == Some(b'?')
        && params.len() == 1
        && matches!(
            (params[0], final_character),
            ([1001 | 1004 | 1015], 'l') | ([1004], 'h')
        )
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use prost::Message as _;

    use super::*;
    use crate::terminal::{
        WireEchoAcknowledgement, WireHostBytes, WireHostDifference, WireHostOperation, WireHostSize,
    };

    fn difference(operations: Vec<WireHostOperation>) -> TerminalDifference {
        TerminalDifference::decode(&WireHostDifference { operations }.encode_to_vec()).unwrap()
    }

    fn host_bytes(bytes: &[u8]) -> WireHostOperation {
        WireHostOperation {
            host_bytes: Some(WireHostBytes {
                bytes: bytes.to_vec(),
            }),
            size: None,
            echo_acknowledgement: None,
        }
    }

    #[test]
    fn applies_host_bytes_resize_and_echo_in_wire_order() {
        let initial = TerminalState::new(80, 24).unwrap();
        let next = initial
            .apply(&difference(vec![
                host_bytes("A中Z".as_bytes()),
                WireHostOperation {
                    host_bytes: None,
                    size: Some(WireHostSize {
                        columns: 81,
                        rows: 25,
                    }),
                    echo_acknowledgement: None,
                },
                WireHostOperation {
                    host_bytes: None,
                    size: None,
                    echo_acknowledgement: Some(WireEchoAcknowledgement { number: 9 }),
                },
                host_bytes(b"!"),
            ]))
            .unwrap();

        assert_eq!(next.screen.size(), (25, 81));
        assert_eq!(next.screen.cell(0, 0).unwrap().contents(), "A");
        assert_eq!(next.screen.cell(0, 1).unwrap().contents(), "中");
        assert!(next.screen.cell(0, 2).unwrap().is_wide_continuation());
        assert_eq!(next.screen.cell(0, 3).unwrap().contents(), "Z");
        assert_eq!(next.screen.cell(0, 4).unwrap().contents(), "!");
        assert_eq!(next.echo_acknowledgement(), Some(9));
        assert_eq!(initial.screen.size(), (24, 80));
        assert_eq!(initial.echo_acknowledgement(), None);
        assert!(!initial.screen.cell(0, 0).unwrap().has_contents());

        let later = next.apply(&difference(vec![host_bytes(b"later")])).unwrap();
        assert_eq!(later.echo_acknowledgement(), Some(9));
    }

    #[test]
    fn rejects_decreasing_echo_acknowledgements() {
        let acknowledged = TerminalState::new(80, 24)
            .unwrap()
            .apply(&difference(vec![WireHostOperation {
                host_bytes: None,
                size: None,
                echo_acknowledgement: Some(WireEchoAcknowledgement { number: 9 }),
            }]))
            .unwrap();

        assert_eq!(
            acknowledged
                .apply(&difference(vec![WireHostOperation {
                    host_bytes: None,
                    size: None,
                    echo_acknowledgement: Some(WireEchoAcknowledgement { number: 8 }),
                }]))
                .unwrap_err(),
            TerminalError::InvalidOperation
        );
    }

    #[test]
    fn preserves_bounded_combining_characters() {
        let state = TerminalState::new(80, 24)
            .unwrap()
            .apply(&difference(vec![host_bytes("e\u{301}".as_bytes())]))
            .unwrap();
        assert_eq!(state.screen.cell(0, 0).unwrap().contents(), "e\u{301}");

        let eight_scalars = format!("e{}", "\u{301}".repeat(7));
        assert!(
            TerminalState::new(80, 24)
                .unwrap()
                .apply(&difference(vec![host_bytes(eight_scalars.as_bytes())]))
                .is_ok()
        );
        let nine_scalars = format!("e{}", "\u{301}".repeat(8));
        assert_eq!(
            TerminalState::new(80, 24)
                .unwrap()
                .apply(&difference(vec![host_bytes(nine_scalars.as_bytes())]))
                .unwrap_err(),
            TerminalError::TooManyScalarsInCell
        );
    }

    #[test]
    fn preserves_valid_replacement_character_and_cursor_position() {
        let state = TerminalState::new(80, 24)
            .unwrap()
            .apply(&difference(vec![host_bytes(
                "BEFORE-\u{fffd}-AFTER".as_bytes(),
            )]))
            .unwrap();

        assert_eq!(state.screen.contents(), "BEFORE-\u{fffd}-AFTER");
        assert_eq!(state.screen.cell(0, 7).unwrap().contents(), "\u{fffd}");
        assert_eq!(state.screen.cursor_position(), (0, 14));

        let continued = state.apply(&difference(vec![host_bytes(b"Z")])).unwrap();
        assert_eq!(continued.screen.contents(), "BEFORE-\u{fffd}-AFTERZ");
        assert_eq!(continued.screen.cursor_position(), (0, 15));
    }

    #[test]
    fn preserves_printable_unicode_categories_without_special_cases() {
        let samples = [
            ("A", 1),         // ASCII
            ("é", 1),         // precomposed Latin
            ("Ω", 1),         // Greek
            ("م", 1),         // Arabic
            ("क", 1),         // Devanagari
            ("中", 2),        // CJK
            ("🙂", 2),        // supplementary-plane emoji
            ("\u{10437}", 1), // supplementary-plane letter
            ("\u{e000}", 1),  // private-use scalar
            ("\u{fffd}", 1),  // replacement character
            ("\u{a0}", 1),    // nonbreaking space
        ];

        for (sample, width) in samples {
            let text = format!("X{sample}Y");
            let state = TerminalState::new(80, 24)
                .unwrap()
                .apply(&difference(vec![host_bytes(text.as_bytes())]))
                .unwrap();
            assert_eq!(state.screen.cell(0, 1).unwrap().contents(), sample);
            assert_eq!(state.screen.cursor_position(), (0, 2 + width));
            assert_eq!(state.screen.cell(0, 1 + width).unwrap().contents(), "Y");
        }

        for cluster in ["e\u{301}", "x\u{200d}", "x\u{fe0f}"] {
            let state = TerminalState::new(80, 24)
                .unwrap()
                .apply(&difference(vec![host_bytes(cluster.as_bytes())]))
                .unwrap();
            assert_eq!(state.screen.cell(0, 0).unwrap().contents(), cluster);
            assert_eq!(state.screen.cursor_position(), (0, 1));
        }
    }

    #[test]
    fn rejects_del_control_even_after_printable_text() {
        let stable = TerminalState::new(80, 24)
            .unwrap()
            .apply(&difference(vec![host_bytes(b"stable")]))
            .unwrap();

        for bytes in [b"\x7f".as_slice(), b"X\x7fY".as_slice()] {
            assert_eq!(
                stable
                    .apply(&difference(vec![host_bytes(bytes)]))
                    .unwrap_err(),
                TerminalError::UnsupportedControl
            );
            assert_eq!(stable.screen.contents(), "stable");
        }
    }

    #[test]
    fn still_rejects_unhandled_c1_characters() {
        let state = TerminalState::new(80, 24).unwrap();
        assert_eq!(
            state
                .apply(&difference(vec![host_bytes("X\u{80}Y".as_bytes())]))
                .unwrap_err(),
            TerminalError::UnsupportedControl
        );
        assert_eq!(state.screen.contents(), "");
    }

    #[test]
    fn unicode_fallback_abort_regression_is_bounded_and_failure_atomic() {
        let stable = TerminalState::new(80, 24)
            .unwrap()
            .apply(&difference(vec![host_bytes(b"stable")]))
            .unwrap();

        for combining_marks in 0..8 {
            let cell = format!("e{}", "\u{301}".repeat(combining_marks));
            assert!(
                stable
                    .apply(&difference(vec![
                        host_bytes(b"\r"),
                        host_bytes(cell.as_bytes())
                    ]))
                    .is_ok()
            );
        }

        let rejected = format!("e{}", "\u{301}".repeat(8));
        assert_eq!(
            stable
                .apply(&difference(vec![
                    host_bytes(b"\r"),
                    host_bytes(rejected.as_bytes())
                ]))
                .unwrap_err(),
            TerminalError::TooManyScalarsInCell
        );
        assert_eq!(stable.screen.contents(), "stable");
    }

    #[test]
    fn rejects_invalid_incomplete_unsupported_and_unrepresentable_input() {
        let state = TerminalState::new(80, 24).unwrap();
        assert_eq!(
            state
                .apply(&difference(vec![host_bytes(&[0xff])]))
                .unwrap_err(),
            TerminalError::InvalidUtf8
        );
        assert_eq!(
            state
                .apply(&difference(vec![host_bytes(b"\x1b[")]))
                .unwrap_err(),
            TerminalError::IncompleteSequence
        );
        assert_eq!(
            state
                .apply(&difference(vec![host_bytes(b"\x1b[999z")]))
                .unwrap_err(),
            TerminalError::UnsupportedCsi {
                intermediate: None,
                parameter: Some(999),
                final_character: 'z'
            }
        );
        let too_many_bytes = format!("e{}", "\u{1ab0}".repeat(7));
        assert_eq!(
            state
                .apply(&difference(vec![host_bytes(too_many_bytes.as_bytes())]))
                .unwrap_err(),
            TerminalError::CellEncodingTooLarge
        );
    }

    #[test]
    fn validates_terminal_dimensions_and_visible_cells() {
        assert_eq!(
            TerminalState::new(0, 24).unwrap_err(),
            TerminalError::InvalidTerminalSize
        );
        assert_eq!(
            TerminalState::new(MAX_TERMINAL_COLUMNS + 1, 24).unwrap_err(),
            TerminalError::InvalidTerminalSize
        );
        assert_eq!(
            TerminalState::new(500, 200).unwrap().screen.size(),
            (200, 500)
        );
    }

    #[test]
    fn drops_system_effects_from_authoritative_state() {
        let state = TerminalState::new(80, 24)
            .unwrap()
            .apply(&difference(vec![host_bytes(
                b"A\x07\x1b]2;secret title\x07\x1b]52;c;c2VjcmV0\x07B",
            )]))
            .unwrap();
        assert_eq!(state.screen.contents(), "AB");
    }

    #[test]
    fn accepts_only_verified_external_policy_resets() {
        let state = TerminalState::new(80, 24).unwrap();
        for reset in [b"\x1b[?1001l".as_slice(), b"\x1b[?1004l", b"\x1b[?1015l"] {
            assert!(state.apply(&difference(vec![host_bytes(reset)])).is_ok());
        }
        assert_eq!(
            state
                .apply(&difference(vec![host_bytes(b"\x1b[?1015h")]))
                .unwrap_err(),
            TerminalError::UnsupportedCsi {
                intermediate: Some(b'?'),
                parameter: Some(1015),
                final_character: 'h'
            }
        );
    }

    #[test]
    fn suppresses_verified_focus_reporting_toggles() {
        let state = TerminalState::new(80, 24).unwrap();
        for toggle in [b"\x1b[?1004h".as_slice(), b"\x1b[?1004l"] {
            let next = state.apply(&difference(vec![host_bytes(toggle)])).unwrap();
            assert_eq!(next.screen.contents(), state.screen.contents());
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn ordered_ascii_chunks_converge_to_the_concatenated_update(
            chunks in prop::collection::vec(
                prop::collection::vec(0x20_u8..0x7f, 0..32),
                1..16,
            )
        ) {
            let operations = chunks.iter().map(|chunk| host_bytes(chunk)).collect();
            let chunked = TerminalState::new(80, 24)
                .unwrap()
                .apply(&difference(operations))
                .unwrap();
            let concatenated = chunks.concat();
            let single = TerminalState::new(80, 24)
                .unwrap()
                .apply(&difference(vec![host_bytes(&concatenated)]))
                .unwrap();

            prop_assert_eq!(
                chunked.screen.state_formatted(),
                single.screen.state_formatted()
            );
        }
    }
}
