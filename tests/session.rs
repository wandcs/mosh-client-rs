use std::net::Ipv4Addr;
use std::time::Duration;

use mosh_client::{
    Bootstrap, Session, SessionCommandError, SessionError, SessionExit, SessionState,
};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn bootstrap() -> Bootstrap {
    Bootstrap::parse(
        Ipv4Addr::LOCALHOST,
        b"MOSH CONNECT 65000 4NeCCgvZFe2RnPgrcU1PQw",
    )
    .unwrap()
}

#[test]
fn public_session_validates_initial_size_without_starting_a_task() {
    runtime().block_on(async {
        let result = Session::connect(bootstrap(), 0, 24).await;
        assert!(matches!(result, Err(SessionError::InvalidTerminalSize)));
    });
}

#[test]
fn public_session_rejects_invalid_commands_before_queueing_them() {
    runtime().block_on(async {
        let (session, task) = Session::connect(bootstrap(), 80, 24).await.unwrap();

        assert_eq!(
            session.send_input(vec![0; 64 * 1024 + 1]).await,
            Err(SessionCommandError::InputTooLarge)
        );
        assert_eq!(
            session.resize(0, 24).await,
            Err(SessionCommandError::InvalidTerminalSize)
        );

        session.cancel();
        assert_eq!(Box::pin(task.run()).await.unwrap(), SessionExit::Cancelled);
    });
}

#[test]
fn public_cancellation_is_idempotent_and_closes_lifecycle_and_output() {
    runtime().block_on(async {
        let (mut session, task) = Session::connect(bootstrap(), 80, 24).await.unwrap();
        assert_eq!(session.state(), SessionState::Connecting);
        let task = tokio::spawn(task.run());

        session.cancel();
        session.cancel();
        assert_eq!(
            session.send_input(b"ignored".to_vec()).await,
            Err(SessionCommandError::Closed)
        );

        let exit = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("cancelled public Session did not stop")
            .unwrap()
            .unwrap();
        assert_eq!(exit, SessionExit::Cancelled);
        assert_eq!(session.state_changed().await, SessionState::Closed);
        assert_eq!(session.state(), SessionState::Closed);
        assert_eq!(session.next_output().await, None);
    });
}

#[test]
fn dropping_the_public_owner_stops_its_task() {
    runtime().block_on(async {
        let (session, task) = Session::connect(bootstrap(), 80, 24).await.unwrap();
        drop(session);

        let exit = Box::pin(tokio::time::timeout(Duration::from_secs(1), task.run()))
            .await
            .expect("ownerless public Session did not stop")
            .unwrap();
        assert_eq!(exit, SessionExit::OwnerDropped);
    });
}
