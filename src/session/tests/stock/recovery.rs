use super::*;

use std::net::{SocketAddr, SocketAddrV4};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Instant as StdInstant;

use tokio::net::UdpSocket;

use crate::limits::MAX_DATAGRAM_BYTES;

#[test]
#[ignore = "requires a locally installed stock mosh-server 1.4.0"]
fn stock_1_4_0_prediction_modes_preserve_convergence_and_adapt_to_latency() {
    const CASES: [(&str, &str, &str, u64); 3] = [
        ("60500:60509", "60510:60519", "60720:60729", 0),
        ("60520:60529", "60530:60539", "60730:60739", 40),
        ("60540:60549", "60550:60559", "60740:60749", 80),
    ];

    assert_stock_1_4_0("mosh-server");
    let runtime = stock_runtime();

    for (baseline_ports, always_ports, adaptive_ports, one_way_delay_ms) in CASES {
        let (bootstrap, mut server) = start_stock_server(baseline_ports);
        let baseline = runtime.block_on(measure_echo_latency(
            bootstrap,
            Duration::from_millis(one_way_delay_ms),
            PredictionMode::Never,
        ));
        assert!(server.terminate(), "stock server fixture did not clean up");

        let minimum = Duration::from_millis(one_way_delay_ms.saturating_mul(2));
        assert!(
            baseline.iter().all(|sample| *sample >= minimum),
            "a non-predictive echo bypassed the configured bidirectional relay delay"
        );

        let (bootstrap, mut server) = start_stock_server(always_ports);
        let always = runtime.block_on(measure_echo_latency(
            bootstrap,
            Duration::from_millis(one_way_delay_ms),
            PredictionMode::Always,
        ));
        assert!(server.terminate(), "stock server fixture did not clean up");

        let baseline_median = median_millis(&baseline);
        let always_median = median_millis(&always);
        if one_way_delay_ms > 0 {
            assert!(
                always_median.saturating_mul(2) < baseline_median,
                "always prediction did not materially reduce the median visible latency"
            );
        }

        let (bootstrap, mut server) = start_stock_server(adaptive_ports);
        let adaptive = runtime.block_on(measure_echo_latency(
            bootstrap,
            Duration::from_millis(one_way_delay_ms),
            PredictionMode::Adaptive,
        ));
        assert!(server.terminate(), "stock server fixture did not clean up");
        let adaptive_median = median_millis(&adaptive);
        if one_way_delay_ms == 0 {
            assert!(
                adaptive_median.saturating_mul(2) >= baseline_median,
                "adaptive prediction treated the low-delay link as slow"
            );
        } else {
            assert!(
                adaptive_median.saturating_mul(2) < baseline_median,
                "adaptive prediction did not activate on the delayed link"
            );
        }
        eprintln!(
            "echo latency: one-way delay={one_way_delay_ms} ms, never={:?} (median={baseline_median} ms), always={:?} (median={always_median} ms), adaptive={:?} (median={adaptive_median} ms)",
            baseline.iter().map(Duration::as_millis).collect::<Vec<_>>(),
            always.iter().map(Duration::as_millis).collect::<Vec<_>>(),
            adaptive.iter().map(Duration::as_millis).collect::<Vec<_>>(),
        );
    }
}

async fn measure_echo_latency(
    bootstrap: Bootstrap,
    one_way_delay: Duration,
    prediction_mode: PredictionMode,
) -> Vec<Duration> {
    let relay = UdpRelay::start_with_delay(bootstrap.server_addr(), one_way_delay).await;
    let bootstrap = bootstrap.with_test_server_addr(relay.endpoint());
    let (commands, mut output, task) =
        start_private_session_with_prediction_mode(bootstrap, prediction_mode).await;
    let mut projection = vt100::Parser::new(24, 80, 0);

    wait_for_screen(&mut output, &mut projection, "MOSH_SESSION> ").await;
    let mut visible = String::from("MOSH_SESSION> ");
    let mut samples = Vec::new();
    for byte in b"latencyprobe" {
        let started = StdInstant::now();
        commands
            .send(SessionCommand::Input(vec![*byte]))
            .await
            .expect("Session command queue closed during latency measurement");
        visible.push(char::from(*byte));
        wait_for_screen_with_timeout(
            &mut output,
            &mut projection,
            &visible,
            Duration::from_secs(5),
        )
        .await;
        samples.push(started.elapsed());
    }

    commands
        .send(SessionCommand::Input(
            b"\x15printf 'LATENCY_AUTHORITY_CONVERGED\\n'\n".to_vec(),
        ))
        .await
        .expect("Session command queue closed before latency convergence check");
    wait_for_screen_with_timeout(
        &mut output,
        &mut projection,
        "LATENCY_AUTHORITY_CONVERGED",
        Duration::from_secs(5),
    )
    .await;

    cancel_session(&commands, task).await;
    samples
}

fn median_millis(samples: &[Duration]) -> u128 {
    let mut millis = samples.iter().map(Duration::as_millis).collect::<Vec<_>>();
    millis.sort_unstable();
    millis[millis.len() / 2]
}

#[test]
#[ignore = "requires a locally installed stock mosh-server 1.4.0"]
fn stock_1_4_0_public_prediction_survives_total_udp_loss_after_confirmation() {
    const ONE_WAY_DELAY: Duration = Duration::from_millis(40);
    const ALWAYS_WARMUP: &str = "alwayswarmup";
    const NEVER_WARMUP: &str = "neverwarmup";

    assert_stock_1_4_0("mosh-server");
    let (always_bootstrap, mut always_server) = start_stock_server("60760:60779");
    let (never_bootstrap, mut never_server) = start_stock_server("60780:60799");

    stock_runtime().block_on(async {
        let mut always = PublicPredictionSession::start(
            always_bootstrap,
            PredictionMode::Always,
            ONE_WAY_DELAY,
        )
        .await;
        let mut never = PublicPredictionSession::start(
            never_bootstrap,
            PredictionMode::Never,
            ONE_WAY_DELAY,
        )
        .await;

        let confirmation = always
            .establish_confirmed_epoch(ALWAYS_WARMUP, ONE_WAY_DELAY)
            .await;
        let always_prefix = &ALWAYS_WARMUP[..confirmation.typed];
        let never_prefix = &NEVER_WARMUP[..confirmation.typed];
        never.type_authoritative(never_prefix).await;

        always.settle_authority().await;
        never.settle_authority().await;
        always.relay.pause();
        never.relay.pause();
        always.quiesce_after_pause(ONE_WAY_DELAY).await;
        never.quiesce_after_pause(ONE_WAY_DELAY).await;
        let always_before = always.relay_snapshot();
        let never_before = never.relay_snapshot();

        let always_started = StdInstant::now();
        always.send_byte(b'x').await;
        let predicted_marker = format!("{always_prefix}x");
        always
            .wait_for_visible(&predicted_marker, Duration::from_millis(250))
            .await;
        let predicted_after = always_started.elapsed();

        never.send_byte(b'y').await;
        assert!(
            tokio::time::timeout(Duration::from_millis(250), never.session.next_output())
                .await
                .is_err(),
            "Never emitted terminal output while all UDP traffic was blocked"
        );

        always.wait_for_dropped_packet(always_before.dropped).await;
        never.wait_for_dropped_packet(never_before.dropped).await;
        let always_during = always.relay_snapshot();
        let never_during = never.relay_snapshot();
        assert_relay_blocked("Always", always_before, always_during);
        assert_relay_blocked("Never", never_before, never_during);
        assert!(
            !never.contents().contains(&predicted_marker),
            "Always prediction leaked into the Never Session"
        );

        always.relay.resume();
        never.relay.resume();
        always
            .converge_with_marker("ALWAYS_AUTHORITY_CONVERGED")
            .await;
        never
            .converge_with_marker("NEVER_AUTHORITY_CONVERGED")
            .await;
        assert!(
            !always.contents().contains("NEVER_AUTHORITY_CONVERGED"),
            "Never authority leaked into the Always Session"
        );
        assert!(
            !never.contents().contains("ALWAYS_AUTHORITY_CONVERGED"),
            "Always authority leaked into the Never Session"
        );

        eprintln!(
            "public full-loss prediction: confirmation byte={}, confirmation latency={} ms, outage prediction latency={} ms, Always dropped={}, Never dropped={}",
            confirmation.typed,
            confirmation.latency.as_millis(),
            predicted_after.as_millis(),
            always_during.dropped - always_before.dropped,
            never_during.dropped - never_before.dropped,
        );

        always.cancel().await;
        never.cancel().await;
    });

    assert!(
        always_server.terminate(),
        "Always stock server fixture did not clean up"
    );
    assert!(
        never_server.terminate(),
        "Never stock server fixture did not clean up"
    );
}

struct PredictionConfirmation {
    typed: usize,
    latency: Duration,
}

struct PublicPredictionSession {
    session: Session,
    task: JoinHandle<Result<SessionExit, SessionError>>,
    projection: vt100::Parser,
    relay: UdpRelay,
}

impl PublicPredictionSession {
    async fn start(
        bootstrap: Bootstrap,
        prediction_mode: PredictionMode,
        one_way_delay: Duration,
    ) -> Self {
        let relay = UdpRelay::start_with_delay(bootstrap.server_addr(), one_way_delay).await;
        let bootstrap = bootstrap.with_test_server_addr(relay.endpoint());
        let (mut session, task) =
            Session::connect_with_prediction_mode(bootstrap, 80, 24, prediction_mode)
                .await
                .expect("public prediction Session setup failed");
        let task = tokio::spawn(task.run());
        let mut projection = vt100::Parser::new(24, 80, 0);
        wait_for_public_screen(&mut session, &mut projection, "MOSH_SESSION> ").await;
        Self {
            session,
            task,
            projection,
            relay,
        }
    }

    fn contents(&self) -> String {
        self.projection.screen().contents()
    }

    async fn send_byte(&self, byte: u8) {
        self.session
            .send_input(vec![byte])
            .await
            .expect("public prediction Session rejected input");
    }

    async fn wait_for_visible(&mut self, marker: &str, timeout: Duration) {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(tokio::time::Instant::now())
                .unwrap_or_default();
            let output = tokio::time::timeout(remaining, self.session.next_output())
                .await
                .unwrap_or_else(|_| panic!("timed out waiting for public prediction {marker:?}"))
                .expect("public prediction Session output closed");
            self.projection.process(&output);
            if self.contents().contains(marker) {
                return;
            }
        }
    }

    async fn establish_confirmed_epoch(
        &mut self,
        input: &str,
        prediction_threshold: Duration,
    ) -> PredictionConfirmation {
        let mut visible = String::from("MOSH_SESSION> ");
        for (index, byte) in input.bytes().enumerate() {
            let started = StdInstant::now();
            self.send_byte(byte).await;
            visible.push(char::from(byte));
            self.wait_for_visible(&visible, Duration::from_secs(5))
                .await;
            let latency = started.elapsed();
            if latency < prediction_threshold {
                return PredictionConfirmation {
                    typed: index + 1,
                    latency,
                };
            }
        }
        panic!("Always never exposed a confirmed prediction through public Session output");
    }

    async fn type_authoritative(&mut self, input: &str) {
        let mut visible = String::from("MOSH_SESSION> ");
        for byte in input.bytes() {
            self.send_byte(byte).await;
            visible.push(char::from(byte));
            self.wait_for_visible(&visible, Duration::from_secs(5))
                .await;
        }
    }

    async fn settle_authority(&mut self) {
        tokio::time::sleep(Duration::from_millis(300)).await;
        while let Ok(Some(output)) =
            tokio::time::timeout(Duration::from_millis(100), self.session.next_output()).await
        {
            self.projection.process(&output);
        }
    }

    async fn quiesce_after_pause(&mut self, one_way_delay: Duration) {
        tokio::time::sleep(one_way_delay + one_way_delay + Duration::from_millis(100)).await;
        while let Ok(Some(output)) =
            tokio::time::timeout(Duration::from_millis(50), self.session.next_output()).await
        {
            self.projection.process(&output);
        }
    }

    fn relay_snapshot(&self) -> RelaySnapshot {
        let traffic = self.relay.traffic();
        RelaySnapshot {
            sent: traffic.sent.iter().sum(),
            received: traffic.received.iter().sum(),
            dropped: self.relay.dropped_packets(),
        }
    }

    async fn wait_for_dropped_packet(&self, before: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.relay.dropped_packets() == before {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("relay did not drop outage traffic");
    }

    async fn converge_with_marker(&mut self, marker: &str) {
        let command = format!("\x15printf '{marker}\\n'\n");
        self.session
            .send_input(command.into_bytes())
            .await
            .expect("public prediction Session rejected convergence input");
        self.wait_for_visible(marker, Duration::from_secs(10)).await;
        self.session
            .request_repaint()
            .await
            .expect("public prediction Session rejected convergence repaint");
        let repaint = tokio::time::timeout(Duration::from_secs(2), self.session.next_output())
            .await
            .expect("public prediction convergence repaint timed out")
            .expect("public prediction output closed before convergence repaint");
        let mut authoritative = vt100::Parser::new(24, 80, 0);
        authoritative.process(&repaint);
        assert!(authoritative.screen().contents().contains(marker));
        self.projection = authoritative;
    }

    async fn cancel(self) {
        self.session.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), self.task)
                .await
                .expect("cancelled public prediction Session did not stop")
                .expect("public prediction Session task panicked")
                .expect("public prediction Session failed"),
            SessionExit::Cancelled
        );
    }
}

#[derive(Clone, Copy)]
struct RelaySnapshot {
    sent: usize,
    received: usize,
    dropped: usize,
}

fn assert_relay_blocked(mode: &str, before: RelaySnapshot, during: RelaySnapshot) {
    assert_eq!(
        during.sent, before.sent,
        "relay delivered the {mode} outage input to the stock server"
    );
    assert_eq!(
        during.received, before.received,
        "relay delivered stock output to the {mode} Session during the outage"
    );
}

#[test]
#[ignore = "requires a locally installed stock mosh-server 1.4.0"]
fn stock_1_4_0_private_session_recovers_after_outage_and_source_port_change() {
    assert_stock_1_4_0("mosh-server");
    let (bootstrap, mut server) = start_stock_server("60460:60479");
    let server_addr = bootstrap.server_addr();

    let runtime = stock_runtime();
    runtime.block_on(async {
        let relay = UdpRelay::start(server_addr).await;
        let bootstrap = bootstrap.with_test_server_addr(relay.endpoint());
        let (commands, mut output, task) = start_private_session(bootstrap).await;
        let mut projection = vt100::Parser::new(24, 80, 0);

        wait_for_screen(&mut output, &mut projection, "MOSH_SESSION> ").await;
        commands
            .send(SessionCommand::Input(
                b"printf 'BEFORE_RECOVERY_OUTAGE\\n'\n".to_vec(),
            ))
            .await
            .expect("Session command queue closed before outage baseline");
        wait_for_screen(&mut output, &mut projection, "BEFORE_RECOVERY_OUTAGE").await;

        relay.pause();
        commands
            .send(SessionCommand::Input(
                b"printf 'RECOVERED_AFTER_OUTAGE\\n'\n".to_vec(),
            ))
            .await
            .expect("Session command queue closed during outage");
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        assert!(
            !task.is_finished(),
            "Session stopped during a network outage"
        );
        assert!(
            relay.dropped_packets() > 0,
            "relay did not observe traffic during the outage"
        );

        relay.switch_source_port();
        relay.resume();
        wait_for_screen_with_timeout(
            &mut output,
            &mut projection,
            "RECOVERED_AFTER_OUTAGE",
            Duration::from_secs(10),
        )
        .await;

        commands
            .send(SessionCommand::Input(
                b"printf 'AFTER_SOURCE_CHANGE_OK\\n'\n".to_vec(),
            ))
            .await
            .expect("Session command queue closed after source-port change");
        wait_for_screen_with_timeout(
            &mut output,
            &mut projection,
            "AFTER_SOURCE_CHANGE_OK",
            Duration::from_secs(10),
        )
        .await;

        commands
            .send(SessionCommand::Repaint)
            .await
            .expect("Session command queue closed before recovery repaint");
        let repaint = tokio::time::timeout(Duration::from_secs(2), output.recv())
            .await
            .expect("recovery repaint was not delivered")
            .expect("Session output queue closed before recovery repaint");
        let mut replacement = vt100::Parser::new(24, 80, 0);
        replacement.process(&repaint);
        assert!(
            replacement
                .screen()
                .contents()
                .contains("AFTER_SOURCE_CHANGE_OK")
        );

        let traffic = relay.traffic();
        assert!(traffic.sent[0] > 0, "first relay source sent no datagrams");
        assert!(
            traffic.received[0] > 0,
            "first relay source received no datagrams"
        );
        assert!(traffic.sent[1] > 0, "second relay source sent no datagrams");
        assert!(
            traffic.received[1] > 0,
            "stock server did not reply to the changed source port"
        );

        cancel_session(&commands, task).await;
    });

    assert!(server.terminate(), "stock server fixture did not clean up");
}

#[test]
#[ignore = "requires a locally installed stock mosh-server 1.4.0"]
fn stock_1_4_0_public_reachability_reports_outage_reply_loss_and_recovery() {
    assert_stock_1_4_0("mosh-server");
    let (bootstrap, mut server) = start_stock_server("60700:60719");
    let server_addr = bootstrap.server_addr();

    stock_runtime().block_on(async {
        let relay = UdpRelay::start(server_addr).await;
        let bootstrap = bootstrap.with_test_server_addr(relay.endpoint());
        let (mut session, session_task) = Session::connect(bootstrap, 80, 24)
            .await
            .expect("public Session setup failed");
        let mut reachability = session.subscribe_reachability();
        let task = tokio::spawn(session_task.run());
        let mut projection = vt100::Parser::new(24, 80, 0);

        wait_for_public_screen(&mut session, &mut projection, "MOSH_SESSION> ").await;
        wait_for_reachability(&mut reachability, SessionReachability::Responsive).await;

        let replayed_datagram = relay.last_server_datagram();
        relay.pause();
        for _ in 0..6 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            relay.inject_to_client(&replayed_datagram).await;
        }
        wait_for_reachability(
            &mut reachability,
            SessionReachability::Interrupted {
                reason: SessionInterruption::NoRecentContact,
            },
        )
        .await;
        assert!(!task.is_finished(), "outage ended an active Session");

        relay.resume();
        wait_for_reachability(&mut reachability, SessionReachability::Responsive).await;
        session
            .send_input(b"printf 'REACHABILITY_RECOVERED\n'\n".to_vec())
            .await
            .expect("Session rejected input after recovery");
        wait_for_public_screen(&mut session, &mut projection, "REACHABILITY_RECOVERED").await;

        session
            .send_input(
                b"(i=0; while [ \"$i\" -lt 20 ]; do printf 'CONTACT_TICK_%s\\n' \"$i\"; i=$((i+1)); sleep 1; done) &\n"
                    .to_vec(),
            )
            .await
            .expect("Session rejected remote contact generator");
        wait_for_public_screen(&mut session, &mut projection, "CONTACT_TICK_0").await;
        relay.pause_client_to_server();
        wait_for_reachability(
            &mut reachability,
            SessionReachability::Interrupted {
                reason: SessionInterruption::NoRecentReply,
            },
        )
        .await;
        assert!(!task.is_finished(), "reply loss ended an active Session");

        relay.resume();
        wait_for_reachability(&mut reachability, SessionReachability::Responsive).await;
        session.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .expect("cancelled Session did not stop")
                .expect("Session task panicked")
                .expect("Session task failed"),
            SessionExit::Cancelled
        );
    });

    assert!(server.terminate(), "stock server fixture did not clean up");
}

async fn wait_for_reachability(
    reachability: &mut SessionReachabilityWatch,
    expected: SessionReachability,
) {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            if reachability.current() == expected {
                return;
            }
            reachability
                .changed()
                .await
                .expect("reachability observer closed before expected state");
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for reachability {expected:?}"));
}

#[test]
#[ignore = "requires a locally installed stock mosh-server 1.4.0"]
fn stock_1_4_0_public_session_reports_interruption_after_server_disappears() {
    assert_stock_1_4_0("mosh-server");
    let (bootstrap, mut server) = start_stock_server("60480:60499");

    let runtime = stock_runtime();
    runtime.block_on(async {
        let (mut session, session_task) = Session::connect(bootstrap, 80, 24)
            .await
            .expect("public Session setup failed");
        let mut reachability = session.subscribe_reachability();
        let task = tokio::spawn(session_task.run());
        let mut projection = vt100::Parser::new(24, 80, 0);

        wait_for_public_screen(&mut session, &mut projection, "MOSH_SESSION> ").await;
        wait_for_reachability(&mut reachability, SessionReachability::Responsive).await;
        assert!(server.signal_kill(), "failed to kill stock server");
        tokio::time::timeout(Duration::from_secs(2), async {
            while !server.has_exited() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("stock server did not exit after termination signal");
        wait_for_reachability(
            &mut reachability,
            SessionReachability::Interrupted {
                reason: SessionInterruption::NoRecentContact,
            },
        )
        .await;
        assert!(
            !task.is_finished(),
            "Session stopped solely because the server became silent"
        );
        assert_eq!(session.state(), SessionState::Active);

        session
            .request_repaint()
            .await
            .expect("Session command queue closed after server disappearance");
        let mut replacement = vt100::Parser::new(24, 80, 0);
        let repaint = tokio::time::timeout(Duration::from_secs(2), session.next_output())
            .await
            .expect("repaint timed out after server disappearance")
            .expect("output closed after server disappearance");
        replacement.process(&repaint);
        assert!(replacement.screen().contents().contains("MOSH_SESSION> "));

        session.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .expect("cancelled Session did not stop")
                .expect("Session task panicked")
                .expect("Session task failed"),
            SessionExit::Cancelled
        );
    });

    assert!(server.terminate(), "stock server fixture did not clean up");
}

#[test]
#[ignore = "requires a locally installed stock mosh-server 1.4.0"]
fn stock_1_4_0_public_sessions_isolate_packets_state_and_lifecycle() {
    assert_stock_1_4_0("mosh-server");
    let (bootstrap_a, mut server_a) = start_stock_server("60600:60619");
    let (bootstrap_b, mut server_b) = start_stock_server("60620:60639");
    let (bootstrap_replacement, mut server_replacement) = start_stock_server("60640:60659");

    stock_runtime().block_on(exercise_public_session_isolation(
        bootstrap_a,
        bootstrap_b,
        bootstrap_replacement,
    ));

    assert!(server_a.terminate(), "first stock server did not clean up");
    assert!(server_b.terminate(), "second stock server did not clean up");
    assert!(
        server_replacement.terminate(),
        "replacement stock server did not clean up"
    );
}

async fn exercise_public_session_isolation(
    bootstrap_a: Bootstrap,
    bootstrap_b: Bootstrap,
    bootstrap_replacement: Bootstrap,
) {
    let mut session_a = PublicStockSession::start(bootstrap_a, PredictionMode::Always).await;
    let mut session_b = PublicStockSession::start(bootstrap_b, PredictionMode::Never).await;
    assert_ne!(session_a.endpoint(), session_b.endpoint());

    session_a.send_and_wait("SESSION_A_ONLY").await;
    let old_a_datagram = session_a.last_server_datagram();
    session_b.inject(&old_a_datagram).await;
    assert!(!session_b.task_is_finished());

    session_b.send_and_wait("SESSION_B_ONLY").await;
    assert!(!session_a.contents().contains("SESSION_B_ONLY"));
    assert!(!session_b.contents().contains("SESSION_A_ONLY"));

    session_a.resize_and_report(90, 25).await;
    session_b.resize_and_report(70, 20).await;
    session_a.repaint_and_wait("SESSION_A_ONLY", "25 90").await;
    session_b.repaint_and_wait("SESSION_B_ONLY", "20 70").await;

    session_a.cancel().await;
    session_a.reject_late_packet(&old_a_datagram).await;
    session_b.prove_timer_continues().await;
    session_b.send_and_wait("SESSION_B_AFTER_A_CANCEL").await;

    let mut replacement =
        PublicStockSession::start(bootstrap_replacement, PredictionMode::Adaptive).await;
    assert_ne!(session_a.endpoint(), replacement.endpoint());
    assert_ne!(session_b.endpoint(), replacement.endpoint());
    replacement.inject(&old_a_datagram).await;
    assert!(!replacement.task_is_finished());
    replacement.send_and_wait("REPLACEMENT_SESSION_ONLY").await;
    assert!(!replacement.contents().contains("SESSION_A_ONLY"));

    session_b.close().await;
    assert!(!replacement.task_is_finished());
    replacement.send_and_wait("REPLACEMENT_AFTER_B_CLOSE").await;
    replacement.drop_owner().await;
}

struct PublicStockSession {
    session: Option<Session>,
    task: Option<JoinHandle<Result<SessionExit, SessionError>>>,
    projection: vt100::Parser,
    relay: UdpRelay,
}

impl PublicStockSession {
    async fn start(bootstrap: Bootstrap, prediction_mode: PredictionMode) -> Self {
        let relay = UdpRelay::start(bootstrap.server_addr()).await;
        let bootstrap = bootstrap.with_test_server_addr(relay.endpoint());
        let (mut session, task) =
            Session::connect_with_prediction_mode(bootstrap, 80, 24, prediction_mode)
                .await
                .expect("public Session setup failed");
        let task = tokio::spawn(task.run());
        let mut projection = vt100::Parser::new(24, 80, 0);
        wait_for_public_screen(&mut session, &mut projection, "MOSH_SESSION> ").await;
        assert_eq!(session.state(), SessionState::Active);
        Self {
            session: Some(session),
            task: Some(task),
            projection,
            relay,
        }
    }

    fn endpoint(&self) -> SocketAddrV4 {
        self.relay.endpoint()
    }

    fn contents(&self) -> String {
        self.projection.screen().contents()
    }

    fn task_is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    fn last_server_datagram(&self) -> Vec<u8> {
        self.relay.last_server_datagram()
    }

    async fn inject(&self, datagram: &[u8]) {
        self.relay.inject_to_client(datagram).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    async fn send_and_wait(&mut self, marker: &str) {
        let command = format!("printf '{marker}\\n'\n");
        let session = self.session.as_mut().expect("Session owner missing");
        session
            .send_input(command.into_bytes())
            .await
            .expect("public Session rejected input");
        wait_for_public_screen(session, &mut self.projection, marker).await;
    }

    async fn resize_and_report(&mut self, columns: u16, rows: u16) {
        let session = self.session.as_mut().expect("Session owner missing");
        session
            .resize(columns.into(), rows.into())
            .await
            .expect("public Session rejected resize");
        self.projection.screen_mut().set_size(rows, columns);
        session
            .send_input(b"stty size\n".to_vec())
            .await
            .expect("public Session rejected size query");
        let expected = format!("{rows} {columns}");
        wait_for_public_screen(session, &mut self.projection, &expected).await;
    }

    async fn repaint_and_wait(&mut self, marker: &str, size: &str) {
        let session = self.session.as_mut().expect("Session owner missing");
        session
            .request_repaint()
            .await
            .expect("public Session rejected repaint");
        let (rows, columns) = self.projection.screen().size();
        let mut replacement = vt100::Parser::new(rows, columns, 0);
        wait_for_public_screen(session, &mut replacement, marker).await;
        assert!(replacement.screen().contents().contains(size));
        self.projection = replacement;
    }

    async fn cancel(&mut self) {
        self.session
            .as_ref()
            .expect("Session owner missing")
            .cancel();
        assert_eq!(self.join_task().await, SessionExit::Cancelled);
        assert_eq!(
            self.session
                .as_ref()
                .expect("Session owner missing")
                .state(),
            SessionState::Closed
        );
    }

    async fn close(&mut self) {
        let mut session = self.session.take().expect("Session owner missing");
        session.close();
        let drain = tokio::spawn(async move {
            while session.next_output().await.is_some() {}
            session
        });
        assert_eq!(self.join_task().await, SessionExit::LocalClosed);
        self.session = Some(drain.await.expect("Session output drain panicked"));
        assert_eq!(
            self.session
                .as_ref()
                .expect("Session owner missing")
                .state(),
            SessionState::Closed
        );
    }

    async fn reject_late_packet(&mut self, datagram: &[u8]) {
        let session = self.session.as_mut().expect("Session owner missing");
        drain_public_output(session).await;
        self.relay.inject_to_client(datagram).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(session.next_output().await, None);
    }

    async fn prove_timer_continues(&self) {
        let before = self.relay.total_client_datagrams();
        tokio::time::sleep(Duration::from_millis(3_500)).await;
        assert!(self.relay.total_client_datagrams() > before);
        assert!(!self.task_is_finished());
    }

    async fn drop_owner(&mut self) {
        drop(self.session.take().expect("Session owner missing"));
        assert_eq!(self.join_task().await, SessionExit::OwnerDropped);
    }

    async fn join_task(&mut self) -> SessionExit {
        let task = self.task.take().expect("Session task missing");
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("public Session task did not stop")
            .expect("public Session task panicked")
            .expect("public Session task failed")
    }
}

async fn drain_public_output(session: &mut Session) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while session.next_output().await.is_some() {}
    })
    .await
    .expect("closed public Session output did not finish");
}

#[derive(Clone, Copy)]
struct RelayTraffic {
    sent: [usize; 2],
    received: [usize; 2],
}

#[derive(Clone)]
struct RelayControl {
    client_addr: Arc<Mutex<Option<SocketAddr>>>,
    last_server_datagram: Arc<Mutex<Option<Vec<u8>>>>,
    drop_client_to_server: Arc<AtomicBool>,
    drop_server_to_client: Arc<AtomicBool>,
    active_source: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
    one_way_delay: Duration,
}

struct UdpRelay {
    endpoint: SocketAddrV4,
    downstream: Arc<UdpSocket>,
    client_addr: Arc<Mutex<Option<SocketAddr>>>,
    last_server_datagram: Arc<Mutex<Option<Vec<u8>>>>,
    drop_client_to_server: Arc<AtomicBool>,
    drop_server_to_client: Arc<AtomicBool>,
    active_source: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
    sent: [Arc<AtomicUsize>; 2],
    received: [Arc<AtomicUsize>; 2],
    tasks: Vec<JoinHandle<()>>,
}

impl UdpRelay {
    async fn start(server_addr: SocketAddrV4) -> Self {
        Self::start_with_delay(server_addr, Duration::ZERO).await
    }

    async fn start_with_delay(server_addr: SocketAddrV4, one_way_delay: Duration) -> Self {
        let downstream = Arc::new(
            UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
                .await
                .expect("failed to bind relay client endpoint"),
        );
        let endpoint = socket_addr_v4(
            downstream
                .local_addr()
                .expect("relay client endpoint has no local address"),
        );
        let upstreams = [
            Arc::new(
                UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
                    .await
                    .expect("failed to bind first relay source"),
            ),
            Arc::new(
                UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
                    .await
                    .expect("failed to bind second relay source"),
            ),
        ];
        for upstream in &upstreams {
            upstream
                .connect(server_addr)
                .await
                .expect("failed to connect relay source to stock server");
        }
        assert_ne!(
            upstreams[0].local_addr().unwrap().port(),
            upstreams[1].local_addr().unwrap().port(),
            "relay sources unexpectedly share a UDP port"
        );

        let drop_client_to_server = Arc::new(AtomicBool::new(false));
        let drop_server_to_client = Arc::new(AtomicBool::new(false));
        let active_source = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let sent = [Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0))];
        let received = [Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0))];
        let client_addr = Arc::new(Mutex::new(None));
        let last_server_datagram = Arc::new(Mutex::new(None));
        let control = RelayControl {
            client_addr: Arc::clone(&client_addr),
            last_server_datagram: Arc::clone(&last_server_datagram),
            drop_client_to_server: Arc::clone(&drop_client_to_server),
            drop_server_to_client: Arc::clone(&drop_server_to_client),
            active_source: Arc::clone(&active_source),
            dropped: Arc::clone(&dropped),
            one_way_delay,
        };

        let mut tasks = Vec::with_capacity(3);
        tasks.push(tokio::spawn(forward_client_datagrams(
            Arc::clone(&downstream),
            [Arc::clone(&upstreams[0]), Arc::clone(&upstreams[1])],
            control.clone(),
            [Arc::clone(&sent[0]), Arc::clone(&sent[1])],
        )));
        for (index, upstream) in upstreams.into_iter().enumerate() {
            tasks.push(tokio::spawn(forward_server_datagrams(
                upstream,
                Arc::clone(&downstream),
                control.clone(),
                Arc::clone(&received[index]),
            )));
        }

        Self {
            endpoint,
            downstream,
            client_addr,
            last_server_datagram,
            drop_client_to_server,
            drop_server_to_client,
            active_source,
            dropped,
            sent,
            received,
            tasks,
        }
    }

    const fn endpoint(&self) -> SocketAddrV4 {
        self.endpoint
    }

    fn pause(&self) {
        self.drop_client_to_server.store(true, Ordering::SeqCst);
        self.drop_server_to_client.store(true, Ordering::SeqCst);
    }

    fn pause_client_to_server(&self) {
        self.drop_client_to_server.store(true, Ordering::SeqCst);
        self.drop_server_to_client.store(false, Ordering::SeqCst);
    }

    fn resume(&self) {
        self.drop_client_to_server.store(false, Ordering::SeqCst);
        self.drop_server_to_client.store(false, Ordering::SeqCst);
    }

    fn switch_source_port(&self) {
        self.active_source.store(1, Ordering::SeqCst);
    }

    fn dropped_packets(&self) -> usize {
        self.dropped.load(Ordering::SeqCst)
    }

    fn traffic(&self) -> RelayTraffic {
        RelayTraffic {
            sent: [
                self.sent[0].load(Ordering::SeqCst),
                self.sent[1].load(Ordering::SeqCst),
            ],
            received: [
                self.received[0].load(Ordering::SeqCst),
                self.received[1].load(Ordering::SeqCst),
            ],
        }
    }

    fn total_client_datagrams(&self) -> usize {
        self.sent
            .iter()
            .map(|count| count.load(Ordering::SeqCst))
            .sum()
    }

    fn last_server_datagram(&self) -> Vec<u8> {
        self.last_server_datagram
            .lock()
            .expect("relay server-datagram lock poisoned")
            .clone()
            .expect("relay captured no server datagram")
    }

    async fn inject_to_client(&self, datagram: &[u8]) {
        let client = self
            .client_addr
            .lock()
            .expect("relay client lock poisoned")
            .expect("relay has not observed its client");
        let sent = self
            .downstream
            .send_to(datagram, client)
            .await
            .expect("failed to inject datagram into relay client");
        assert_eq!(sent, datagram.len(), "relay injection was incomplete");
    }
}

impl Drop for UdpRelay {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn forward_client_datagrams(
    downstream: Arc<UdpSocket>,
    upstreams: [Arc<UdpSocket>; 2],
    control: RelayControl,
    sent: [Arc<AtomicUsize>; 2],
) {
    let mut datagram = [0_u8; MAX_DATAGRAM_BYTES];
    while let Ok((length, source)) = downstream.recv_from(&mut datagram).await {
        {
            let mut known_client = control
                .client_addr
                .lock()
                .expect("relay client lock poisoned");
            if known_client.is_none() {
                *known_client = Some(source);
            }
            if *known_client != Some(source) {
                continue;
            }
        }
        if control.drop_client_to_server.load(Ordering::SeqCst) {
            control.dropped.fetch_add(1, Ordering::SeqCst);
            continue;
        }
        tokio::time::sleep(control.one_way_delay).await;
        let index = control.active_source.load(Ordering::SeqCst).min(1);
        if upstreams[index].send(&datagram[..length]).await.is_err() {
            break;
        }
        sent[index].fetch_add(1, Ordering::SeqCst);
    }
}

async fn forward_server_datagrams(
    upstream: Arc<UdpSocket>,
    downstream: Arc<UdpSocket>,
    control: RelayControl,
    received: Arc<AtomicUsize>,
) {
    let mut datagram = [0_u8; MAX_DATAGRAM_BYTES];
    while let Ok(length) = upstream.recv(&mut datagram).await {
        if control.drop_server_to_client.load(Ordering::SeqCst) {
            control.dropped.fetch_add(1, Ordering::SeqCst);
            continue;
        }
        let client = *control
            .client_addr
            .lock()
            .expect("relay client lock poisoned");
        let Some(client) = client else {
            continue;
        };
        *control
            .last_server_datagram
            .lock()
            .expect("relay server-datagram lock poisoned") = Some(datagram[..length].to_vec());
        tokio::time::sleep(control.one_way_delay).await;
        if downstream
            .send_to(&datagram[..length], client)
            .await
            .is_err()
        {
            break;
        }
        received.fetch_add(1, Ordering::SeqCst);
    }
}

fn socket_addr_v4(address: SocketAddr) -> SocketAddrV4 {
    match address {
        SocketAddr::V4(address) => address,
        SocketAddr::V6(_) => panic!("loopback relay unexpectedly bound an IPv6 address"),
    }
}
