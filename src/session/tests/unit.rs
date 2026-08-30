use super::super::*;

use crate::crypto::SessionKey;
use crate::limits::INITIAL_ATTACHMENT_TIMEOUT_MS;
use crate::test_support::STOCK_1_4_0_KEY_BYTES;

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
            graceful_close: _graceful_close,
            state: _state,
            reachability: _reachability,
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
            graceful_close: _graceful_close,
            state: _state,
            reachability: _reachability,
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

#[test]
fn cancellation_preempts_a_blocked_final_output_drain() {
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
            graceful_close: _graceful_close,
            state: _state,
            reachability: _reachability,
        } = channels;
        let task = tokio::spawn(async move {
            driver
                .finish_graceful_close(SessionExit::RemoteClosed)
                .await
        });

        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        cancellation.send_replace(true);
        let close = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("final-output drain ignored cancellation")
            .unwrap()
            .unwrap();

        assert_eq!(close, SessionExit::Cancelled);
    });
}

#[test]
fn initial_attachment_timeout_is_reported_before_any_peer_state() {
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
        driver.started_at = Instant::now() - Duration::from_millis(INITIAL_ATTACHMENT_TIMEOUT_MS);
        let mut state = channels.state;

        assert_eq!(
            Box::pin(SessionTask { driver }.run()).await,
            Err(SessionError::ConnectionTimeout)
        );
        assert!(state.changed().await.is_ok());
        assert_eq!(*state.borrow_and_update(), SessionState::Closed);
    });
}

#[test]
fn graceful_close_retransmits_after_loss_and_accepts_a_later_acknowledgement() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let server = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = server.local_addr().unwrap().port();
        let bootstrap_text = format!("MOSH CONNECT {port} 4NeCCgvZFe2RnPgrcU1PQw");
        let bootstrap = Bootstrap::parse(Ipv4Addr::LOCALHOST, bootstrap_text.as_bytes()).unwrap();
        let (session, task) = Session::connect(bootstrap, 80, 24).await.unwrap();
        let task = tokio::spawn(task.run());
        session
            .send_input(b"ordered-before-close".to_vec())
            .await
            .unwrap();
        session.close();

        let key = SessionKey::new(STOCK_1_4_0_KEY_BYTES);
        let mut receiver = PacketReceiver::new(&key, Direction::ClientToServer);
        let codec = PacketCodec::new(&key);
        let mut inbound = [0_u8; MAX_DATAGRAM_BYTES];

        let (first, client_addr) = receive_shutdown(&server, &mut receiver, &mut inbound).await;
        assert!(
            first
                .state_difference
                .windows(b"ordered-before-close".len())
                .any(|window| window == b"ordered-before-close"),
            "close difference omitted input accepted before close"
        );
        let false_shutdown = TransportInstruction {
            protocol_version: PROTOCOL_VERSION,
            base_state: 0,
            new_state: SHUTDOWN_STATE,
            acknowledged_state: SHUTDOWN_STATE,
            discard_before_state: 0,
            state_difference: Vec::new(),
            chaff: vec![0],
        };
        let wrong_key = SessionKey::new([0x42; 16]);
        let wrong_codec = PacketCodec::new(&wrong_key);
        send_server_instruction(&server, &wrong_codec, client_addr, &false_shutdown).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !task.is_finished(),
            "unauthenticated shutdown stopped Session"
        );

        let (retry, retry_addr) = receive_shutdown(&server, &mut receiver, &mut inbound).await;
        assert_eq!(retry_addr, client_addr);
        assert_eq!(retry.base_state, first.base_state);
        assert_eq!(retry.new_state, SHUTDOWN_STATE);
        assert_eq!(retry.state_difference, first.state_difference);

        let acknowledgement = TransportInstruction {
            protocol_version: PROTOCOL_VERSION,
            base_state: 0,
            new_state: 1,
            acknowledged_state: SHUTDOWN_STATE,
            discard_before_state: 0,
            state_difference: Vec::new(),
            chaff: vec![0],
        };
        send_server_instruction(&server, &codec, client_addr, &acknowledgement).await;

        let exit = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("graceful close ignored a later authenticated acknowledgement")
            .unwrap()
            .unwrap();
        assert_eq!(exit, SessionExit::LocalClosed);
    });
}

#[test]
fn simultaneous_authenticated_shutdown_is_acknowledged_and_reports_local_close() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let server = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = server.local_addr().unwrap().port();
        let bootstrap_text = format!("MOSH CONNECT {port} 4NeCCgvZFe2RnPgrcU1PQw");
        let bootstrap = Bootstrap::parse(Ipv4Addr::LOCALHOST, bootstrap_text.as_bytes()).unwrap();
        let (session, task) = Session::connect(bootstrap, 80, 24).await.unwrap();
        let task = tokio::spawn(task.run());
        session.close();

        let key = SessionKey::new(STOCK_1_4_0_KEY_BYTES);
        let mut receiver = PacketReceiver::new(&key, Direction::ClientToServer);
        let codec = PacketCodec::new(&key);
        let mut inbound = [0_u8; MAX_DATAGRAM_BYTES];
        let (_local_shutdown, client_addr) =
            receive_shutdown(&server, &mut receiver, &mut inbound).await;
        let simultaneous = TransportInstruction {
            protocol_version: PROTOCOL_VERSION,
            base_state: 0,
            new_state: SHUTDOWN_STATE,
            acknowledged_state: SHUTDOWN_STATE,
            discard_before_state: 0,
            state_difference: Vec::new(),
            chaff: vec![0],
        };
        send_server_instruction(&server, &codec, client_addr, &simultaneous).await;

        let acknowledgement = loop {
            let (instruction, _) = receive_instruction(&server, &mut receiver, &mut inbound).await;
            if !instruction.is_shutdown() && instruction.acknowledged_state == SHUTDOWN_STATE {
                break instruction;
            }
        };
        assert_ne!(acknowledgement.new_state, SHUTDOWN_STATE);
        let exit = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("simultaneous graceful close did not stop")
            .unwrap()
            .unwrap();
        assert_eq!(exit, SessionExit::LocalClosed);
    });
}

async fn receive_shutdown(
    socket: &UdpSocket,
    receiver: &mut PacketReceiver,
    inbound: &mut [u8],
) -> (TransportInstruction, SocketAddr) {
    loop {
        let (instruction, source) = receive_instruction(socket, receiver, inbound).await;
        if instruction.is_shutdown() {
            return (instruction, source);
        }
    }
}

async fn receive_instruction(
    socket: &UdpSocket,
    receiver: &mut PacketReceiver,
    inbound: &mut [u8],
) -> (TransportInstruction, SocketAddr) {
    let (length, source) = tokio::time::timeout(Duration::from_secs(2), socket.recv_from(inbound))
        .await
        .expect("client did not send the expected graceful-close instruction")
        .unwrap();
    let opened = receiver.open(&mut inbound[..length]).unwrap();
    let decoded = fragment::decode(opened.plaintext).unwrap();
    let instruction = TransportInstruction::decode_zlib(decoded.body).unwrap();
    (instruction, source)
}

async fn send_server_instruction(
    socket: &UdpSocket,
    codec: &PacketCodec,
    client_addr: SocketAddr,
    instruction: &TransportInstruction,
) {
    let compressed = instruction.encode_zlib().unwrap();
    let mut plaintext = [0_u8; MAX_DATAGRAM_BYTES];
    let plaintext_len = fragment::encode(0, None, 0, 0, true, &compressed, &mut plaintext).unwrap();
    let mut datagram = [0_u8; MAX_DATAGRAM_BYTES];
    let datagram_len = codec
        .seal(
            Direction::ServerToClient,
            0,
            &plaintext[..plaintext_len],
            &mut datagram,
        )
        .unwrap();
    socket
        .send_to(&datagram[..datagram_len], client_addr)
        .await
        .unwrap();
}
