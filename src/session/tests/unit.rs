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

        cancellation.send_replace(true);
        let close = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("cancelled Session did not stop")
            .unwrap()
            .unwrap();

        assert_eq!(close, SessionExit::Cancelled);
    });
}
