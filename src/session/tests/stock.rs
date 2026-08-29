use super::super::*;

use std::net::Ipv4Addr;
use std::process::{Command, Stdio};
use tokio::task::JoinHandle;

use zeroize::Zeroize as _;

#[path = "stock/recovery.rs"]
mod recovery;

#[test]
#[ignore = "requires a locally installed stock mosh-server 1.4.0"]
fn stock_1_4_0_public_session_drives_an_interactive_shell() {
    assert_stock_server_version();
    let (bootstrap, mut server) = start_stock_server("60400:60419");

    let runtime = stock_runtime();
    runtime.block_on(async {
        let (mut session, session_task) = Session::connect(bootstrap, 80, 24)
            .await
            .expect("public Session setup failed");
        let task = tokio::spawn(session_task.run());
        let mut projection = vt100::Parser::new(24, 80, 0);

        wait_for_public_screen(&mut session, &mut projection, "MOSH_SESSION> ").await;
        assert_eq!(session.state(), SessionState::Active);
        session
            .send_input(b"printf 'SESSION_E2E_OK\\n'\n".to_vec())
            .await
            .expect("Session command queue closed before input");
        wait_for_public_screen(&mut session, &mut projection, "SESSION_E2E_OK").await;
        session
            .request_repaint()
            .await
            .expect("Session command queue closed before repaint");
        let repaint = tokio::time::timeout(Duration::from_secs(2), session.next_output())
            .await
            .expect("full repaint was not delivered")
            .expect("Session output queue closed before full repaint");
        let mut replacement = vt100::Parser::new(24, 80, 0);
        replacement.process(&repaint);
        assert!(replacement.screen().contents().contains("SESSION_E2E_OK"));
        session.cancel();
        let exit = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("cancelled public stock Session did not stop")
            .unwrap()
            .expect("public stock Session failed");
        assert_eq!(exit, SessionExit::Cancelled);
    });

    assert!(server.terminate(), "stock server fixture did not clean up");
}

async fn wait_for_public_screen(
    session: &mut Session,
    projection: &mut vt100::Parser,
    marker: &str,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .unwrap_or_default();
        let output = tokio::time::timeout(remaining, session.next_output())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {marker:?}"))
            .expect("public Session output closed before expected screen");
        projection.process(&output);
        if projection.screen().contents().contains(marker) {
            return;
        }
    }
}

#[test]
#[ignore = "requires a locally installed stock mosh-server 1.4.0"]
fn stock_1_4_0_private_session_resizes_the_remote_pty_and_repaints() {
    assert_stock_server_version();
    let (bootstrap, mut server) = start_stock_server("60560:60579");

    let runtime = stock_runtime();
    runtime.block_on(async {
        let (commands, mut output, task) = start_private_session(bootstrap).await;
        let mut projection = vt100::Parser::new(24, 80, 0);

        wait_for_screen(&mut output, &mut projection, "MOSH_SESSION> ").await;
        commands
            .send(SessionCommand::Input(b"stty size\n".to_vec()))
            .await
            .expect("Session command queue closed before baseline size check");
        wait_for_screen(&mut output, &mut projection, "24 80").await;

        commands
            .send(SessionCommand::Resize {
                columns: 100,
                rows: 30,
            })
            .await
            .expect("Session command queue closed before first resize");
        projection.screen_mut().set_size(30, 100);
        commands
            .send(SessionCommand::Input(b"stty size\n".to_vec()))
            .await
            .expect("Session command queue closed after first resize");
        wait_for_screen(&mut output, &mut projection, "30 100").await;

        commands
            .send(SessionCommand::Resize {
                columns: 60,
                rows: 20,
            })
            .await
            .expect("Session command queue closed before second resize");
        projection.screen_mut().set_size(20, 60);
        commands
            .send(SessionCommand::Input(b"stty size\n".to_vec()))
            .await
            .expect("Session command queue closed after second resize");
        wait_for_screen(&mut output, &mut projection, "20 60").await;

        commands
            .send(SessionCommand::Repaint)
            .await
            .expect("Session command queue closed before resized repaint");
        let repaint = tokio::time::timeout(Duration::from_secs(2), output.recv())
            .await
            .expect("resized full repaint was not delivered")
            .expect("Session output queue closed before resized full repaint");
        let mut replacement = vt100::Parser::new(20, 60, 0);
        replacement.process(&repaint);
        assert!(replacement.screen().contents().contains("20 60"));

        cancel_session(&commands, task).await;
    });

    assert!(server.terminate(), "stock server fixture did not clean up");
}

#[test]
#[ignore = "requires local stock mosh-server 1.4.0, tmux, and Vim"]
fn stock_1_4_0_private_session_supports_tmux_vim_and_full_screen_repaint() {
    assert_stock_server_version();
    assert_program_available("tmux", "-V");
    assert_program_available("vim", "--version");
    let (bootstrap, mut server) = start_stock_server("60420:60439");
    let tmux_socket = format!("mosh-client-rs-{}", std::process::id());
    let _tmux = TmuxServerGuard::new(tmux_socket.clone());

    let runtime = stock_runtime();
    runtime.block_on(async {
        let (commands, mut output, task) = start_private_session(bootstrap).await;
        let mut projection = vt100::Parser::new(24, 80, 0);

        wait_for_screen(&mut output, &mut projection, "MOSH_SESSION> ").await;
        exercise_tmux(&commands, &mut output, &mut projection, &tmux_socket).await;
        exercise_vim(&commands, &mut output, &mut projection).await;

        cancel_session(&commands, task).await;
    });

    assert!(server.terminate(), "stock server fixture did not clean up");
}

async fn exercise_tmux(
    commands: &TestSessionCommands,
    output: &mut mpsc::Receiver<Vec<u8>>,
    projection: &mut vt100::Parser,
    tmux_socket: &str,
) {
    for command in [
        format!(
            "tmux -L {tmux_socket} -f /dev/null new-session -d -s fixture \"/usr/bin/env PS1='MOSH_TMUX_ONE> ' /bin/sh -i\"\n"
        ),
        format!(
            "tmux -L {tmux_socket} new-window -d -t fixture \"/usr/bin/env PS1='MOSH_TMUX_TWO> ' /bin/sh -i\"\n"
        ),
        format!("tmux -L {tmux_socket} attach-session -t fixture:0\n"),
    ] {
        commands
            .send(SessionCommand::Input(command.into_bytes()))
            .await
            .expect("Session command queue closed during tmux startup");
    }
    wait_for_screen(output, projection, "MOSH_TMUX_ONE> ").await;
    commands
        .send(SessionCommand::Input(
            b"printf 'TMUX_WINDOW_ONE_OK\\n'\n".to_vec(),
        ))
        .await
        .expect("Session command queue closed before tmux input");
    wait_for_screen(output, projection, "TMUX_WINDOW_ONE_OK").await;
    commands
        .send(SessionCommand::Input(b"\x02n".to_vec()))
        .await
        .expect("Session command queue closed before tmux navigation");
    wait_for_screen(output, projection, "MOSH_TMUX_TWO> ").await;
    commands
        .send(SessionCommand::Input(
            b"printf 'TMUX_WINDOW_TWO_OK\\n'\n".to_vec(),
        ))
        .await
        .expect("Session command queue closed before second tmux window input");
    wait_for_screen(output, projection, "TMUX_WINDOW_TWO_OK").await;
    commands
        .send(SessionCommand::Input(b"\x02p".to_vec()))
        .await
        .expect("Session command queue closed before reverse tmux navigation");
    wait_for_screen(output, projection, "TMUX_WINDOW_ONE_OK").await;
    detach_tmux(commands, output, projection, "tmux detach").await;
    commands
        .send(SessionCommand::Input(
            format!("tmux -L {tmux_socket} attach-session -t fixture:1\n").into_bytes(),
        ))
        .await
        .expect("Session command queue closed before tmux reattach");
    wait_for_screen(output, projection, "TMUX_WINDOW_TWO_OK").await;
    detach_tmux(commands, output, projection, "tmux reattach").await;
}

async fn detach_tmux(
    commands: &TestSessionCommands,
    output: &mut mpsc::Receiver<Vec<u8>>,
    projection: &mut vt100::Parser,
    description: &str,
) {
    commands
        .send(SessionCommand::Input(b"\x02d".to_vec()))
        .await
        .expect("Session command queue closed before tmux detach");
    wait_for_projection(
        output,
        projection,
        description,
        Duration::from_secs(5),
        |screen| {
            !screen.contents().contains("[fixture]") && screen.contents().contains("MOSH_SESSION> ")
        },
    )
    .await;
}

async fn exercise_vim(
    commands: &TestSessionCommands,
    output: &mut mpsc::Receiver<Vec<u8>>,
    projection: &mut vt100::Parser,
) {
    commands
        .send(SessionCommand::Input(
            b"vim -Nu NONE -n -i NONE +'set noswapfile'\n".to_vec(),
        ))
        .await
        .expect("Session command queue closed before Vim startup");
    wait_for_projection(
        output,
        projection,
        "Vim full-screen state",
        Duration::from_secs(5),
        |screen| screen.contents().contains("VIM - Vi IMproved"),
    )
    .await;
    commands
        .send(SessionCommand::Input(b"iEDITOR_SESSION_OK\x1b".to_vec()))
        .await
        .expect("Session command queue closed before Vim input");
    wait_for_screen(output, projection, "EDITOR_SESSION_OK").await;

    commands
        .send(SessionCommand::Repaint)
        .await
        .expect("Session command queue closed before full-screen repaint");
    let repaint = tokio::time::timeout(Duration::from_secs(2), output.recv())
        .await
        .expect("full-screen repaint was not delivered")
        .expect("Session output queue closed before full-screen repaint");
    let mut replacement = vt100::Parser::new(24, 80, 0);
    replacement.process(&repaint);
    assert!(
        replacement
            .screen()
            .contents()
            .contains("EDITOR_SESSION_OK")
    );

    commands
        .send(SessionCommand::Input(b"\x1b:q!\n".to_vec()))
        .await
        .expect("Session command queue closed before Vim exit");
    wait_for_projection(
        output,
        projection,
        "restored primary shell screen",
        Duration::from_secs(5),
        |screen| !screen.alternate_screen() && screen.contents().contains("MOSH_SESSION> "),
    )
    .await;
}

#[test]
#[ignore = "requires a locally installed stock mosh-server 1.4.0"]
fn stock_1_4_0_private_session_preserves_sustained_io_under_output_backpressure() {
    assert_stock_server_version();
    let (bootstrap, mut server) = start_stock_server("60440:60459");

    let runtime = stock_runtime();
    runtime.block_on(async {
        let (commands, mut output, task) = start_private_session(bootstrap).await;
        let mut projection = vt100::Parser::new(24, 80, 0);

        wait_for_screen(&mut output, &mut projection, "MOSH_SESSION> ").await;
        commands
            .send(SessionCommand::Input(
                b"PS1=; stty -echo; payload=\n".to_vec(),
            ))
            .await
            .expect("Session command queue closed before sustained input setup");
        let mut expected_input = String::new();
        for index in 0..128 {
            let chunk = format!("{index:03}");
            expected_input.push_str(&chunk);
            commands
                .send(SessionCommand::Input(
                    format!("payload=\"${{payload}}{chunk}\"\n").into_bytes(),
                ))
                .await
                .expect("Session command queue closed during sustained input");
        }
        commands
            .send(SessionCommand::Input(
                format!(
                    "stty echo; if [ \"$payload\" = \"{expected_input}\" ]; then printf 'SUSTAINED_INPUT_ORDER_OK\\n'; else printf 'SUSTAINED_INPUT_BAD_%s\\n' \"${{#payload}}\"; fi\n"
                )
                .into_bytes(),
            ))
            .await
            .expect("Session command queue closed after sustained input");
        wait_for_screen_with_timeout(
            &mut output,
            &mut projection,
            "SUSTAINED_INPUT_ORDER_OK",
            Duration::from_secs(10),
        )
        .await;

        commands
            .send(SessionCommand::Input(
                b"i=0; while [ \"$i\" -lt 1200 ]; do printf 'SUSTAINED_%04d_abcdefghijklmnopqrstuvwxyz0123456789\\n' \"$i\"; i=$((i+1)); done; printf 'SUSTAINED_DONE_1200\\n'\n".to_vec(),
            ))
            .await
            .expect("Session command queue closed before sustained output");

        tokio::time::sleep(Duration::from_millis(500)).await;
        wait_for_screen_with_timeout(
            &mut output,
            &mut projection,
            "SUSTAINED_DONE_1200",
            Duration::from_secs(10),
        )
        .await;
        commands
            .send(SessionCommand::Repaint)
            .await
            .expect("Session command queue closed before sustained-output repaint");
        let repaint = tokio::time::timeout(Duration::from_secs(2), output.recv())
            .await
            .expect("sustained-output repaint was not delivered")
            .expect("Session output queue closed before sustained-output repaint");
        let mut replacement = vt100::Parser::new(24, 80, 0);
        replacement.process(&repaint);
        assert!(
            replacement
                .screen()
                .contents()
                .contains("SUSTAINED_DONE_1200")
        );

        cancel_session(&commands, task).await;
    });

    assert!(server.terminate(), "stock server fixture did not clean up");
}

fn start_stock_server(port_range: &str) -> (Bootstrap, DetachedProcessGuard) {
    let mut output = Command::new("mosh-server")
        .env("TERM", "xterm-256color")
        .env("LANG", "C.UTF-8")
        .args([
            "new",
            "-i",
            "127.0.0.1",
            "-p",
            port_range,
            "--",
            "/usr/bin/env",
            "PS1=MOSH_SESSION> ",
            "/bin/sh",
            "-i",
        ])
        .output()
        .unwrap_or_else(|error| panic!("failed to start local stock server: {error}"));
    let detached_pid = find_detached_pid(&output)
        .unwrap_or_else(|| panic!("stock server did not report its detached process identifier"));
    let server = DetachedProcessGuard::new(detached_pid);
    assert!(output.status.success(), "stock server bootstrap failed");
    let bootstrap = Bootstrap::parse(Ipv4Addr::LOCALHOST, &output.stdout)
        .unwrap_or_else(|error| panic!("stock bootstrap output was rejected: {error}"));
    output.stdout.zeroize();
    output.stderr.zeroize();
    (bootstrap, server)
}

fn stock_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

async fn start_private_session(
    bootstrap: Bootstrap,
) -> (
    TestSessionCommands,
    mpsc::Receiver<Vec<u8>>,
    JoinHandle<Result<SessionClose, DriverError>>,
) {
    start_private_session_with_prediction(bootstrap, true).await
}

async fn start_private_session_with_prediction(
    bootstrap: Bootstrap,
    prediction_enabled: bool,
) -> (
    TestSessionCommands,
    mpsc::Receiver<Vec<u8>>,
    JoinHandle<Result<SessionClose, DriverError>>,
) {
    let (mut driver, channels) = SessionDriver::connect(bootstrap, 80, 24)
        .await
        .unwrap_or_else(|error| panic!("private Session setup failed: {error:?}"));
    if !prediction_enabled {
        driver.disable_prediction();
    }
    let SessionChannels {
        commands,
        output,
        cancellation,
        ..
    } = channels;
    let task = tokio::spawn(async move {
        let result = driver.run().await;
        if let Err(error) = &result {
            eprintln!("private Session stopped: {error:?}");
        }
        result
    });
    (
        TestSessionCommands {
            commands,
            cancellation,
        },
        output,
        task,
    )
}

async fn cancel_session(
    commands: &TestSessionCommands,
    task: JoinHandle<Result<SessionClose, DriverError>>,
) {
    commands.cancel();
    let close = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("cancelled stock Session did not stop")
        .unwrap()
        .unwrap_or_else(|error| panic!("private Session failed: {error:?}"));
    assert_eq!(close, SessionClose::Cancelled);
}

struct TestSessionCommands {
    commands: mpsc::Sender<SessionCommand>,
    cancellation: watch::Sender<bool>,
}

impl TestSessionCommands {
    async fn send(
        &self,
        command: SessionCommand,
    ) -> Result<(), mpsc::error::SendError<SessionCommand>> {
        self.commands.send(command).await
    }

    fn cancel(&self) {
        self.cancellation.send_replace(true);
    }
}

async fn wait_for_screen(
    output: &mut mpsc::Receiver<Vec<u8>>,
    projection: &mut vt100::Parser,
    marker: &str,
) {
    wait_for_screen_with_timeout(output, projection, marker, Duration::from_secs(5)).await;
}

async fn wait_for_screen_with_timeout(
    output: &mut mpsc::Receiver<Vec<u8>>,
    projection: &mut vt100::Parser,
    marker: &str,
    timeout: Duration,
) {
    wait_for_projection(output, projection, marker, timeout, |screen| {
        screen.contents().contains(marker)
    })
    .await;
}

async fn wait_for_projection(
    output: &mut mpsc::Receiver<Vec<u8>>,
    projection: &mut vt100::Parser,
    description: &str,
    timeout: Duration,
    mut predicate: impl FnMut(&vt100::Screen) -> bool,
) {
    tokio::time::timeout(timeout, async {
        loop {
            let paint = output
                .recv()
                .await
                .unwrap_or_else(|| panic!("Session output queue closed before {description}"));
            projection.process(&paint);
            if predicate(projection.screen()) {
                break;
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "stock Session did not reach {description:?}; visible screen: {:?}",
            projection.screen().contents()
        )
    });
}

fn assert_program_available(program: &str, version_argument: &str) {
    let status = Command::new(program)
        .arg(version_argument)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap_or_else(|error| panic!("fixture requires local {program}: {error}"));
    assert!(status.success(), "fixture requires local {program}");
}

fn assert_stock_server_version() {
    let version = Command::new("mosh-server")
        .arg("--version")
        .output()
        .unwrap_or_else(|error| panic!("failed to run local stock server: {error}"));
    assert!(
        version.status.success()
            && (version.stdout.windows(10).any(|part| part == b"mosh 1.4.0")
                || version.stderr.windows(10).any(|part| part == b"mosh 1.4.0")),
        "fixture requires stock mosh-server 1.4.0"
    );
}

fn find_detached_pid(output: &std::process::Output) -> Option<u32> {
    find_pid(&output.stdout).or_else(|| find_pid(&output.stderr))
}

fn find_pid(bytes: &[u8]) -> Option<u32> {
    const MARKER: &[u8] = b"pid = ";
    let start = bytes
        .windows(MARKER.len())
        .position(|window| window == MARKER)?
        + MARKER.len();
    let digit_count = bytes[start..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    let digits = &bytes[start..start + digit_count];
    if digits.is_empty() {
        return None;
    }
    let pid = digits.iter().try_fold(0_u32, |value, byte| {
        value.checked_mul(10)?.checked_add(u32::from(*byte - b'0'))
    })?;
    (pid > 1).then_some(pid)
}

struct DetachedProcessGuard {
    pid: u32,
    terminated: bool,
}

impl DetachedProcessGuard {
    const fn new(pid: u32) -> Self {
        Self {
            pid,
            terminated: false,
        }
    }

    fn signal_terminate(&self) -> bool {
        self.has_exited()
            || Command::new("kill")
                .arg("-TERM")
                .arg(self.pid.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
    }

    fn has_exited(&self) -> bool {
        use std::path::Path;

        !Path::new(&format!("/proc/{}", self.pid)).exists()
    }

    fn terminate(&mut self) -> bool {
        use std::path::Path;
        use std::thread;

        if self.terminated || !Path::new(&format!("/proc/{}", self.pid)).exists() {
            self.terminated = true;
            return true;
        }
        if Command::new("kill")
            .arg("-TERM")
            .arg(self.pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            return false;
        }
        for _ in 0..100 {
            if !Path::new(&format!("/proc/{}", self.pid)).exists() {
                self.terminated = true;
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(self.pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        for _ in 0..150 {
            if !Path::new(&format!("/proc/{}", self.pid)).exists() {
                self.terminated = true;
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

impl Drop for DetachedProcessGuard {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

struct TmuxServerGuard {
    socket_name: String,
}

impl TmuxServerGuard {
    const fn new(socket_name: String) -> Self {
        Self { socket_name }
    }
}

impl Drop for TmuxServerGuard {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .arg("-L")
            .arg(&self.socket_name)
            .arg("kill-server")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
