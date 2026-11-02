use faultdeck_netem::{FaultSpec, Injection, LatencySpec, LossSpec, Target, TcCommand};
use ipnet::IpNet;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Recorder {
    commands: Arc<Mutex<Vec<String>>>,
}

impl Recorder {
    fn run(&self, command: &TcCommand) -> faultdeck_netem::Result<()> {
        self.commands.lock().unwrap().push(command.to_string());
        Ok(())
    }
}

fn latency() -> Injection {
    Injection {
        fault: FaultSpec::Latency(LatencySpec { milliseconds: 120 }),
        target: Target::destination_port(IpNet::from_str("192.0.2.0/24").unwrap(), 7000),
    }
}

#[test]
fn plans_targeted_latency_and_rollback() {
    let adapter = faultdeck_netem::NetemAdapter::with_executor(
        "eth1",
        |_: &TcCommand| -> faultdeck_netem::Result<()> { Ok(()) },
    );
    let commands = adapter.plan(&latency()).unwrap();

    assert_eq!(commands[0].to_string(), "tc qdisc del dev eth1 root");
    assert!(commands.contains(&TcCommand {
        program: "tc".into(),
        arguments: vec![
            "qdisc".into(),
            "add".into(),
            "dev".into(),
            "eth1".into(),
            "parent".into(),
            "100:2".into(),
            "handle".into(),
            "50:".into(),
            "netem".into(),
            "delay".into(),
            "120ms".into()
        ]
    }));
    assert_eq!(
        commands.last().unwrap().to_string(),
        "tc qdisc del dev eth1 root"
    );
}

#[test]
fn inject_records_commands_and_runs_rollback() {
    let recorder = Recorder::default();
    let recorder2 = recorder.clone();
    let adapter =
        faultdeck_netem::NetemAdapter::with_executor("lab0", move |command: &TcCommand| {
            recorder2.run(command)
        });
    adapter.inject(&latency()).unwrap();

    let commands = recorder.commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 7);
    assert_eq!(commands.last().unwrap(), "tc qdisc del dev lab0 root");

    adapter.rollback().unwrap();
    let commands = recorder.commands.lock().unwrap().clone();
    assert_eq!(commands.last().unwrap(), "tc qdisc del dev lab0 root");
}

#[test]
fn validates_loss_target_and_ipv6() {
    let adapter = faultdeck_netem::NetemAdapter::with_executor(
        "eth0",
        |_: &TcCommand| -> faultdeck_netem::Result<()> { Ok(()) },
    );
    let loss = Injection {
        fault: FaultSpec::Loss(LossSpec { percent: 2.5 }),
        target: Target::destination(IpNet::from_str("198.51.100.0/24").unwrap()),
    };
    assert!(adapter.plan(&loss).is_ok());

    let empty = Injection {
        fault: FaultSpec::Loss(LossSpec { percent: 2.5 }),
        target: Target::any(),
    };
    assert!(matches!(
        adapter.plan(&empty).unwrap_err(),
        faultdeck_netem::AdapterError::EmptyTarget
    ));

    let zero = Injection {
        fault: FaultSpec::Loss(LossSpec { percent: 0.0 }),
        target: loss.target.clone(),
    };
    assert!(matches!(
        adapter.plan(&zero).unwrap_err(),
        faultdeck_netem::AdapterError::InvalidLossPercent { percent: 0.0 }
    ));

    let ipv6 = Injection {
        fault: loss.fault.clone(),
        target: Target::destination(IpNet::from_str("2001:db8::/32").unwrap()),
    };
    assert!(matches!(
        adapter.plan(&ipv6).unwrap_err(),
        faultdeck_netem::AdapterError::UnsupportedIpv6
    ));
}
