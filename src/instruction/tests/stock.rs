use super::super::*;

#[cfg(target_os = "linux")]
use std::io::Write as _;
#[cfg(target_os = "linux")]
use std::net::{Ipv4Addr, UdpSocket};
#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt as _;
#[cfg(target_os = "linux")]
use std::process::{Child, Command, Stdio};
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use crate::crypto::SessionKey;
#[cfg(target_os = "linux")]
use crate::fragment;
#[cfg(target_os = "linux")]
use crate::limits::MAX_DATAGRAM_BYTES;
#[cfg(target_os = "linux")]
use crate::packet::{Direction, PacketReceiver};
#[cfg(target_os = "linux")]
use crate::test_support::{STOCK_1_4_0_KEY_BYTES, STOCK_1_4_0_KEY_TEXT, assert_stock_1_4_0};

#[cfg(target_os = "linux")]
#[derive(Clone, PartialEq, Message)]
struct ObservedUserDifference {
    #[prost(message, repeated, tag = "1")]
    operations: Vec<ObservedUserOperation>,
}

#[cfg(target_os = "linux")]
#[derive(Clone, PartialEq, Message)]
struct ObservedUserOperation {
    #[prost(message, optional, tag = "2")]
    bytes_operation: Option<ObservedUserBytes>,
}

#[cfg(target_os = "linux")]
#[derive(Clone, PartialEq, Message)]
struct ObservedUserBytes {
    #[prost(bytes = "vec", tag = "4")]
    bytes: Vec<u8>,
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a locally installed stock mosh-client 1.4.0 and util-linux script"]
fn stock_1_4_0_client_input_difference_contains_exact_utf8_bytes() {
    assert_stock_1_4_0("mosh-client");

    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut child = spawn_stock_client(&socket, "stty rows 24 cols 80");
    let key = SessionKey::new(STOCK_1_4_0_KEY_BYTES);
    let mut receiver = PacketReceiver::new(&key, Direction::ClientToServer);

    let initial = receive_stock_instruction(&socket, &mut receiver);
    assert_eq!(initial.new_state, 1);
    child
        .stdin_mut()
        .write_all("x中".as_bytes())
        .expect("failed to write controlled input to stock client");
    child.stdin_mut().flush().unwrap();

    let input = receive_stock_instruction(&socket, &mut receiver);
    let difference = ObservedUserDifference::decode(input.state_difference.as_slice())
        .expect("stock client input difference did not match the observed nested shape");
    let bytes = difference
        .operations
        .iter()
        .filter_map(|operation| operation.bytes_operation.as_ref())
        .flat_map(|operation| operation.bytes.iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(bytes, "x中".as_bytes());

    assert!(child.terminate(), "stock client fixture did not clean up");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a locally installed stock mosh-client 1.4.0 and util-linux script"]
fn stock_1_4_0_client_resize_reuses_the_terminal_size_operation() {
    assert_stock_1_4_0("mosh-client");

    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let setup = concat!(
        "stty rows 24 cols 80; parent=$$; ",
        "(sleep 0.5; stty rows 25 cols 81 </dev/tty; kill -WINCH \"$parent\") &"
    );
    let mut child = spawn_stock_client(&socket, setup);
    let key = SessionKey::new(STOCK_1_4_0_KEY_BYTES);
    let mut receiver = PacketReceiver::new(&key, Direction::ClientToServer);

    let initial = receive_stock_instruction(&socket, &mut receiver);
    let initial_difference =
        TerminalDifference::decode(initial.state_difference.as_slice()).unwrap();
    assert_eq!(terminal_sizes(&initial_difference), [(80, 24)]);

    let resize = receive_stock_instruction(&socket, &mut receiver);
    let resize_difference = TerminalDifference::decode(resize.state_difference.as_slice()).unwrap();
    let observed_sizes = terminal_sizes(&resize_difference);
    assert_eq!(observed_sizes.first(), Some(&(80, 24)));
    assert_eq!(observed_sizes.last(), Some(&(81, 25)));
    assert!(
        observed_sizes
            .iter()
            .all(|size| matches!(size, (80, 24) | (81, 25)))
    );

    assert!(child.terminate(), "stock client fixture did not clean up");
}

#[cfg(target_os = "linux")]
fn spawn_stock_client(socket: &UdpSocket, setup: &str) -> ProcessGroupGuard {
    let command = format!(
        "{setup}\nexec env TERM=xterm-256color LANG=C.UTF-8 MOSH_KEY={STOCK_1_4_0_KEY_TEXT} mosh-client 127.0.0.1 {}",
        socket.local_addr().unwrap().port()
    );
    let child = Command::new("script")
        .args(["-qfec", &command, "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap_or_else(|error| panic!("failed to start stock client fixture: {error}"));
    ProcessGroupGuard::new(child)
}

#[cfg(target_os = "linux")]
fn terminal_sizes(difference: &TerminalDifference) -> Vec<(u32, u32)> {
    difference
        .operations
        .iter()
        .filter_map(|operation| operation.initial_size)
        .map(|size| (size.columns, size.rows))
        .collect()
}

#[cfg(target_os = "linux")]
fn receive_stock_instruction(
    socket: &UdpSocket,
    receiver: &mut PacketReceiver,
) -> TransportInstruction {
    let mut datagram = [0_u8; MAX_DATAGRAM_BYTES];
    let (length, peer) = socket
        .recv_from(&mut datagram)
        .unwrap_or_else(|error| panic!("stock client did not send a datagram: {error}"));
    assert_eq!(peer.ip(), Ipv4Addr::LOCALHOST);
    let packet = receiver
        .open(&mut datagram[..length])
        .unwrap_or_else(|error| panic!("stock client packet was rejected: {error:?}"));
    let decoded_fragment = fragment::decode(packet.plaintext)
        .unwrap_or_else(|error| panic!("stock client fragment was rejected: {error:?}"));
    assert_eq!(decoded_fragment.number, 0);
    assert!(decoded_fragment.is_final);
    TransportInstruction::decode_zlib(decoded_fragment.body)
        .unwrap_or_else(|error| panic!("stock client instruction was rejected: {error:?}"))
}

#[cfg(target_os = "linux")]
struct ProcessGroupGuard {
    child: Child,
    terminated: bool,
}

#[cfg(target_os = "linux")]
impl ProcessGroupGuard {
    const fn new(child: Child) -> Self {
        Self {
            child,
            terminated: false,
        }
    }

    fn stdin_mut(&mut self) -> &mut std::process::ChildStdin {
        self.child.stdin.as_mut().expect("fixture stdin is open")
    }

    fn terminate(&mut self) -> bool {
        if self.terminated {
            return true;
        }
        let process_group = format!("-{}", self.child.id());
        let signalled = Command::new("kill")
            .args(["-TERM", "--", &process_group])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok();
        self.terminated = self.child.wait().is_ok();
        signalled && self.terminated
    }
}

#[cfg(target_os = "linux")]
impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        if !self.terminated {
            let process_group = format!("-{}", self.child.id());
            let _ = Command::new("kill")
                .args(["-KILL", "--", &process_group])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = self.child.wait();
        }
    }
}
