use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::time::Duration;

use faultdeck_agent::transport::{connect, send_frame, send_request, ModeB};

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
enum Msg {
    Ping,
    Pong,
}

#[test]
fn mode_a_framed_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = stream;
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "\"Ping\"");
            send_frame(&mut writer, &Msg::Pong).unwrap();
        }
    });

    let mut stream = connect("127.0.0.1", addr.port(), 3, Duration::from_millis(50)).unwrap();
    send_request(&mut stream, &Msg::Ping).unwrap();
    let mut line = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "\"Pong\"");
    thread.join().unwrap();
}

#[test]
fn mode_b_connects_to_controller_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "\"Ping\"");
        }
    });
    let mode_b = ModeB::new("127.0.0.1", addr.port(), 3, Duration::from_millis(50));
    let mut stream = mode_b.connect().unwrap();
    send_request(&mut stream, &Msg::Ping).unwrap();
    thread.join().unwrap();
}

#[test]
fn mode_b_retries_then_succeeds() {
    // Bind, then connect after a short delay (simulating transient unavailable).
    let holder = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = holder.local_addr().unwrap();
    let listener_thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        if let Ok((stream, _)) = holder.accept() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
        }
    });
    let mode_b = ModeB::new("127.0.0.1", addr.port(), 10, Duration::from_millis(100));
    let mut stream = mode_b.connect().unwrap();
    send_request(&mut stream, &Msg::Ping).unwrap();
    listener_thread.join().unwrap();
}
