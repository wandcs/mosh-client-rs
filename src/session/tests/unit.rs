use super::super::*;

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

        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        cancellation.send_replace(true);
        let close = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("cancelled Session did not stop")
            .unwrap()
            .unwrap();

        assert_eq!(close, SessionExit::Cancelled);
    });
}

#[test]
fn cancellation_preempts_a_blocked_output_reservation() {
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
        let (mut driver, channels) = SessionDriver::connect(bootstrap, 80, 24).await.unwrap();
        driver.output_requested = true;
        driver.output_tx.try_send(b"occupied".to_vec()).unwrap();
        let SessionChannels {
            commands: _commands,
            output: _output,
            cancellation,
            state: _state,
        } = channels;
        let task = tokio::spawn(driver.run());

        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        cancellation.send_replace(true);
        let close = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("output-blocked Session ignored cancellation")
            .unwrap()
            .unwrap();

        assert_eq!(close, SessionExit::Cancelled);
    });
}
