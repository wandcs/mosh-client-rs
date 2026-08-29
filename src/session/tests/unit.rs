use super::super::*;

#[test]
fn client_history_encodes_only_operations_after_the_selected_reference() {
    let mut history = ClientHistory::new();
    history
        .append(ClientOperation::Resize {
            columns: 80,
            rows: 24,
        })
        .unwrap();
    history.checkpoint(1);
    history
        .append(ClientOperation::Input(b"hello\n".to_vec()))
        .unwrap();
    history.checkpoint(2);

    assert_eq!(
        history.difference(1, 2).unwrap(),
        encode_client_difference(&[ClientOperation::Input(b"hello\n".to_vec())]).unwrap()
    );
    history.acknowledge(1).unwrap();
    assert_eq!(history.operation_offset, 1);
    assert_eq!(history.operations.len(), 1);
    assert!(!history.checkpoints.contains_key(&0));
}

#[test]
fn unchanged_checkpoint_encodes_an_empty_acknowledgement() {
    let mut history = ClientHistory::new();
    history.checkpoint(1);

    assert_eq!(
        history.difference(0, 1).unwrap(),
        encode_client_difference(&[]).unwrap()
    );
}

#[test]
fn cancellation_closes_the_private_driver_without_a_server() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let bootstrap = Bootstrap::parse(
            Ipv4Addr::LOCALHOST,
            b"MOSH CONNECT 65000 4NeCCgvZFe2RnPgrcU1PQw",
        )
        .unwrap();
        let (driver, channels) = SessionDriver::connect(bootstrap, 80, 24).await.unwrap();
        let SessionChannels {
            commands: _commands,
            output: _output,
            cancellation,
            state: _state,
        } = channels;
        let task = tokio::spawn(driver.run());

        cancellation.send_replace(true);
        let close = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("cancelled Session did not stop")
            .unwrap()
            .unwrap();

        assert_eq!(close, SessionExit::Cancelled);
    });
}
