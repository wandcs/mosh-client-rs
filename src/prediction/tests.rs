use super::*;

fn state(text: &[u8]) -> TerminalState {
    let mut state = TerminalState::new(80, 24).unwrap();
    for byte in text {
        state = state.with_predicted_ascii(*byte).unwrap();
    }
    state
}

fn prediction(mode: PredictionMode) -> LocalPrediction {
    LocalPrediction::new(mode)
}

#[test]
fn first_prediction_stays_hidden_until_stock_echo_confirmation() {
    let authoritative = state(b"prompt> ");
    let mut prediction = prediction(PredictionMode::Always);

    assert!(
        !prediction
            .observe_input(2, b"a", &authoritative, 10)
            .unwrap()
    );
    assert!(
        prediction
            .display(&authoritative)
            .display_equivalent(&authoritative)
    );

    let confirmed = authoritative.with_predicted_ascii(b'a').unwrap();
    prediction
        .observe_authoritative(Some(2), &confirmed)
        .unwrap();
    assert!(prediction.pending.is_empty());
    assert!(prediction.active_epoch);

    assert!(prediction.observe_input(3, b"b", &confirmed, 20).unwrap());
    assert!(
        prediction
            .display(&confirmed)
            .display_equivalent(&state(b"prompt> ab"))
    );
}

#[test]
fn matching_authority_confirms_visible_predictions_without_changing_projection() {
    let authoritative = state(b"a");
    let mut prediction = prediction(PredictionMode::Always);
    prediction.active_epoch = true;
    prediction
        .observe_input(3, b"b", &authoritative, 20)
        .unwrap();
    let displayed = prediction.display(&authoritative).clone();

    prediction
        .observe_authoritative(Some(3), &displayed)
        .unwrap();

    assert!(prediction.active_epoch);
    assert!(prediction.pending.is_empty());
    assert!(
        prediction
            .display(&displayed)
            .display_equivalent(&displayed)
    );
}

#[test]
fn one_divergence_or_control_input_clears_the_visible_projection() {
    let authoritative = state(b"a");
    let mut prediction = prediction(PredictionMode::Always);
    prediction.active_epoch = true;
    prediction
        .observe_input(3, b"b", &authoritative, 20)
        .unwrap();

    prediction
        .observe_authoritative(Some(3), &state(b"x"))
        .unwrap();
    assert!(!prediction.active_epoch);
    assert!(prediction.pending.is_empty());

    prediction.active_epoch = true;
    prediction
        .observe_input(4, b"c", &authoritative, 30)
        .unwrap();
    assert!(
        prediction
            .observe_input(5, b"\x7f", &authoritative, 31)
            .unwrap()
    );
    assert!(!prediction.active_epoch);
    assert!(prediction.pending.is_empty());
}

#[test]
fn paste_and_additional_tentative_input_are_never_speculated() {
    let authoritative = state(b"a");
    let mut prediction = prediction(PredictionMode::Always);

    assert!(
        !prediction
            .observe_input(2, b"b", &authoritative, 10)
            .unwrap()
    );
    assert!(
        !prediction
            .observe_input(3, b"c", &authoritative, 11)
            .unwrap()
    );
    assert_eq!(prediction.pending.len(), 1);
    assert!(
        !prediction
            .observe_input(4, b"pasted", &authoritative, 12)
            .unwrap()
    );
    assert!(prediction.pending.is_empty());
}

#[test]
fn erase_and_mixed_input_never_enter_the_projection() {
    let authoritative = state(b"prompt> ");
    let mut prediction = prediction(PredictionMode::Always);
    prediction.active_epoch = true;

    assert!(
        prediction
            .observe_input(2, b"a", &authoritative, 10)
            .unwrap()
    );
    prediction
        .observe_authoritative(Some(1), &authoritative)
        .unwrap();
    assert!(
        prediction
            .display(&authoritative)
            .display_equivalent(&state(b"prompt> a"))
    );

    assert!(
        prediction
            .observe_input(3, b"\x08", &authoritative, 11)
            .unwrap()
    );
    assert!(prediction.pending.is_empty());
    assert!(!prediction.active_epoch);
    assert!(
        prediction
            .display(&authoritative)
            .display_equivalent(&authoritative)
    );

    prediction.active_epoch = true;
    assert!(
        prediction
            .observe_input(4, b"b", &authoritative, 12)
            .unwrap()
    );
    assert!(
        prediction
            .observe_input(5, b"\x7f", &authoritative, 13)
            .unwrap()
    );
    assert!(prediction.pending.is_empty());
    assert!(!prediction.active_epoch);

    prediction.active_epoch = true;
    assert!(
        prediction
            .observe_input(6, b"c", &authoritative, 14)
            .unwrap()
    );
    assert!(
        prediction
            .observe_input(7, b"d\x08", &authoritative, 15)
            .unwrap()
    );
    assert!(prediction.pending.is_empty());
    assert!(!prediction.active_epoch);
    assert!(
        prediction
            .display(&authoritative)
            .display_equivalent(&authoritative)
    );
}

#[test]
fn prediction_age_and_capacity_are_bounded() {
    let authoritative = state(b"");
    let mut prediction = prediction(PredictionMode::Always);
    prediction.active_epoch = true;

    for state in 1..=MAX_PENDING_PREDICTION_SCALARS {
        assert!(
            prediction
                .observe_input(u64::try_from(state).unwrap(), b"x", &authoritative, 10,)
                .unwrap()
        );
    }
    assert!(
        prediction
            .observe_input(33, b"x", &authoritative, 10)
            .unwrap()
    );
    assert!(prediction.pending.is_empty());
    assert!(!prediction.active_epoch);

    prediction.active_epoch = true;
    prediction
        .observe_input(34, b"x", &authoritative, 20)
        .unwrap();
    assert_eq!(prediction.next_deadline_ms(), Some(10_020));
    assert!(!prediction.expire(10_019));
    assert!(prediction.expire(10_020));
    assert!(prediction.pending.is_empty());
}

#[test]
fn never_keeps_only_authoritative_display_state() {
    let authoritative = state(b"prompt> ");
    let mut prediction = prediction(PredictionMode::Never);

    assert!(
        !prediction
            .observe_input(2, b"a", &authoritative, 10)
            .unwrap()
    );
    assert!(prediction.pending.is_empty());
    assert!(
        prediction
            .display(&authoritative)
            .display_equivalent(&authoritative)
    );
    assert_eq!(prediction.next_deadline_ms(), None);
}

#[test]
fn adaptive_uses_slow_link_hysteresis_without_retracting_visible_input() {
    let authoritative = state(b"prompt> ");
    let mut prediction = prediction(PredictionMode::Adaptive);
    prediction.active_epoch = true;

    assert!(
        !prediction
            .observe_input(2, b"a", &authoritative, 10)
            .unwrap()
    );
    assert!(!prediction.update_policy(10, None));
    assert!(prediction.update_policy(10, Some(31)));
    assert!(
        prediction
            .display(&authoritative)
            .display_equivalent(&state(b"prompt> a"))
    );

    assert!(!prediction.update_policy(11, Some(25)));
    assert!(!prediction.update_policy(12, Some(20)));
    let confirmed = state(b"prompt> a");
    prediction
        .observe_authoritative(Some(2), &confirmed)
        .unwrap();
    assert!(!prediction.update_policy(13, Some(20)));

    assert!(!prediction.observe_input(3, b"b", &confirmed, 14).unwrap());
    assert!(
        prediction
            .display(&confirmed)
            .display_equivalent(&confirmed)
    );
}

#[test]
fn adaptive_temporarily_displays_a_prediction_during_a_glitch() {
    let authoritative = state(b"prompt> ");
    let mut prediction = prediction(PredictionMode::Adaptive);
    prediction.active_epoch = true;

    assert!(
        !prediction
            .observe_input(2, b"a", &authoritative, 100)
            .unwrap()
    );
    assert_eq!(prediction.next_deadline_ms(), Some(350));
    assert!(!prediction.update_policy(349, Some(20)));
    assert!(prediction.update_policy(350, Some(20)));
    assert!(
        prediction
            .display(&authoritative)
            .display_equivalent(&state(b"prompt> a"))
    );

    let confirmed = state(b"prompt> a");
    prediction
        .observe_authoritative(Some(2), &confirmed)
        .unwrap();
    assert!(!prediction.update_policy(351, Some(20)));
    assert!(!prediction.observe_input(3, b"b", &confirmed, 352).unwrap());
}
