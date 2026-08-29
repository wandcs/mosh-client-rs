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
fn stock_1_4_0_prediction_reduces_measured_interactive_echo_latency() {
    const CASES: [(&str, &str, u64); 3] = [
        ("60500:60509", "60510:60519", 0),
        ("60520:60529", "60530:60539", 40),
        ("60540:60549", "60550:60559", 80),
    ];

    assert_stock_server_version();
    let runtime = stock_runtime();

    for (baseline_ports, prediction_ports, one_way_delay_ms) in CASES {
        let (bootstrap, mut server) = start_stock_server(baseline_ports);
        let baseline = runtime.block_on(measure_echo_latency(
            bootstrap,
            Duration::from_millis(one_way_delay_ms),
            false,
        ));
        assert!(server.terminate(), "stock server fixture did not clean up");

        let minimum = Duration::from_millis(one_way_delay_ms.saturating_mul(2));
        assert!(
            baseline.iter().all(|sample| *sample >= minimum),
            "a non-predictive echo bypassed the configured bidirectional relay delay"
        );

        let (bootstrap, mut server) = start_stock_server(prediction_ports);
        let predicted = runtime.block_on(measure_echo_latency(
            bootstrap,
            Duration::from_millis(one_way_delay_ms),
            true,
        ));
        assert!(server.terminate(), "stock server fixture did not clean up");

        let baseline_median = median_millis(&baseline);
        let predicted_median = median_millis(&predicted);
        if one_way_delay_ms > 0 {
            assert!(
                predicted_median.saturating_mul(2) < baseline_median,
                "prediction did not materially reduce the median visible latency"
            );
        }
        eprintln!(
            "echo latency: one-way delay={one_way_delay_ms} ms, baseline={:?} (median={baseline_median} ms), predicted={:?} (median={predicted_median} ms)",
            baseline.iter().map(Duration::as_millis).collect::<Vec<_>>(),
            predicted
                .iter()
                .map(Duration::as_millis)
                .collect::<Vec<_>>(),
        );
    }
}

async fn measure_echo_latency(
    bootstrap: Bootstrap,
    one_way_delay: Duration,
    prediction_enabled: bool,
) -> Vec<Duration> {
    let relay = UdpRelay::start_with_delay(bootstrap.server_addr(), one_way_delay).await;
    let bootstrap = bootstrap.with_test_server_addr(relay.endpoint());
    let (commands, mut output, task) =
        start_private_session_with_prediction(bootstrap, prediction_enabled).await;
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
fn stock_1_4_0_private_session_recovers_after_outage_and_source_port_change() {
    assert_stock_server_version();
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
fn stock_1_4_0_private_session_remains_controllable_after_server_disappears() {
    assert_stock_server_version();
    let (bootstrap, mut server) = start_stock_server("60480:60499");

    let runtime = stock_runtime();
    runtime.block_on(async {
        let (commands, mut output, task) = start_private_session(bootstrap).await;
        let mut projection = vt100::Parser::new(24, 80, 0);

        wait_for_screen(&mut output, &mut projection, "MOSH_SESSION> ").await;
        assert!(
            server.signal_terminate(),
            "failed to terminate stock server"
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            while !server.has_exited() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("stock server did not exit after termination signal");
        tokio::time::sleep(Duration::from_millis(3_500)).await;
        assert!(
            !task.is_finished(),
            "Session stopped solely because the server became silent"
        );

        commands
            .send(SessionCommand::Repaint)
            .await
            .expect("Session command queue closed after server disappearance");
        let mut replacement = vt100::Parser::new(24, 80, 0);
        wait_for_screen_with_timeout(
            &mut output,
            &mut replacement,
            "MOSH_SESSION> ",
            Duration::from_secs(2),
        )
        .await;

        cancel_session(&commands, task).await;
    });

    assert!(server.terminate(), "stock server fixture did not clean up");
}

#[derive(Clone, Copy)]
struct RelayTraffic {
    sent: [usize; 2],
    received: [usize; 2],
}

#[derive(Clone)]
struct RelayControl {
    client_addr: Arc<Mutex<Option<SocketAddr>>>,
    dropping: Arc<AtomicBool>,
    active_source: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
    one_way_delay: Duration,
}

struct UdpRelay {
    endpoint: SocketAddrV4,
    dropping: Arc<AtomicBool>,
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

        let dropping = Arc::new(AtomicBool::new(false));
        let active_source = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let sent = [Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0))];
        let received = [Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0))];
        let client_addr = Arc::new(Mutex::new(None));
        let control = RelayControl {
            client_addr,
            dropping: Arc::clone(&dropping),
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
            dropping,
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
        self.dropping.store(true, Ordering::SeqCst);
    }

    fn resume(&self) {
        self.dropping.store(false, Ordering::SeqCst);
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
        if control.dropping.load(Ordering::SeqCst) {
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
        if control.dropping.load(Ordering::SeqCst) {
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
