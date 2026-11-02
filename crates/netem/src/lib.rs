use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

use ipnet::{IpNet, Ipv4Net};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PILOT_QDISC_HANDLE: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencySpec {
    pub milliseconds: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LossSpec {
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FaultSpec {
    Latency(LatencySpec),
    Loss(LossSpec),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub source: Option<IpNet>,
    pub destination: Option<IpNet>,
    pub destination_port: Option<u16>,
}

impl Target {
    pub fn any() -> Self {
        Self {
            source: None,
            destination: None,
            destination_port: None,
        }
    }

    pub fn destination(network: IpNet) -> Self {
        Self {
            source: None,
            destination: Some(network),
            destination_port: None,
        }
    }

    pub fn destination_port(network: IpNet, port: u16) -> Self {
        Self {
            source: None,
            destination: Some(network),
            destination_port: Some(port),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Injection {
    pub fault: FaultSpec,
    pub target: Target,
}

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("tc command failed: {program} {arguments}: {message}")]
    Command {
        program: String,
        arguments: String,
        message: String,
    },
    #[error("fault target is empty")]
    EmptyTarget,
    #[error("invalid loss percent {percent}; expected 0 < percent <= 100")]
    InvalidLossPercent { percent: f64 },
    #[error("IPv6 target is unsupported by the current tc adapter")]
    UnsupportedIpv6,
}

pub type Result<T> = std::result::Result<T, AdapterError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcCommand {
    pub program: PathBuf,
    pub arguments: Vec<String>,
}

impl fmt::Display for TcCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {}",
            self.program.display(),
            self.arguments.join(" ")
        )
    }
}

pub trait CommandExecutor {
    fn run(&self, command: &TcCommand) -> Result<()>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemCommandExecutor {
    pub dry_run: bool,
}

impl CommandExecutor for SystemCommandExecutor {
    #[cfg(target_os = "linux")]
    fn run(&self, command: &TcCommand) -> Result<()> {
        if self.dry_run {
            return Ok(());
        }

        let output = std::process::Command::new(&command.program)
            .args(&command.arguments)
            .output()
            .map_err(|error| AdapterError::Command {
                program: command.program.display().to_string(),
                arguments: command.arguments.join(" "),
                message: error.to_string(),
            })?;

        if output.status.success() {
            Ok(())
        } else {
            Err(AdapterError::Command {
                program: command.program.display().to_string(),
                arguments: command.arguments.join(" "),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn run(&self, _command: &TcCommand) -> Result<()> {
        if self.dry_run {
            Ok(())
        } else {
            Err(AdapterError::Command {
                program: "tc".into(),
                arguments: String::new(),
                message: "tc is only available on Linux".into(),
            })
        }
    }
}

impl<F> CommandExecutor for F
where
    F: Fn(&TcCommand) -> Result<()>,
{
    fn run(&self, command: &TcCommand) -> Result<()> {
        self(command)
    }
}

#[derive(Debug, Clone)]
pub struct NetemAdapter<E = SystemCommandExecutor> {
    executor: E,
    interface: String,
}

impl NetemAdapter<SystemCommandExecutor> {
    pub fn new(interface: impl Into<String>) -> Self {
        Self::with_executor(interface, SystemCommandExecutor { dry_run: false })
    }

    pub fn dry_run(interface: impl Into<String>) -> Self {
        Self::with_executor(interface, SystemCommandExecutor { dry_run: true })
    }
}

impl<E> NetemAdapter<E>
where
    E: CommandExecutor,
{
    pub fn with_executor(interface: impl Into<String>, executor: E) -> Self {
        Self {
            executor,
            interface: interface.into(),
        }
    }

    pub fn plan(&self, injection: &Injection) -> Result<Vec<TcCommand>> {
        validate(injection)?;
        let mut commands = self.apply_commands(injection);
        commands.push(self.rollback_command());
        Ok(commands)
    }

    pub fn inject(&self, injection: &Injection) -> Result<Vec<TcCommand>> {
        validate(injection)?;
        let commands = self.apply_commands(injection);
        for command in &commands {
            self.executor.run(command)?;
        }
        self.rollback()?;
        Ok(commands)
    }

    pub fn rollback(&self) -> Result<Vec<TcCommand>> {
        let command = self.rollback_command();
        match self.executor.run(&command) {
            Ok(()) => {}
            Err(AdapterError::Command { message, .. })
                if message.contains("Cannot find")
                    || message.contains("Cannot delete")
                    || message.contains("No such file")
                    || message.contains("No such device") => {}
            Err(error) => return Err(error),
        }
        Ok(vec![command])
    }

    fn apply_commands(&self, injection: &Injection) -> Vec<TcCommand> {
        let mut commands = vec![self.rollback_command(), self.root_qdisc()];
        commands.push(self.pilot_filter(injection).expect("validated injection"));
        commands.extend(self.netem_commands(injection));
        commands.push(self.default_qdisc());
        commands.push(self.default_filter());
        commands
    }

    fn command(&self, arguments: Vec<String>) -> TcCommand {
        TcCommand {
            program: PathBuf::from("tc"),
            arguments,
        }
    }

    fn rollback_command(&self) -> TcCommand {
        self.command(vec![
            "qdisc".into(),
            "del".into(),
            "dev".into(),
            self.interface.clone(),
            "root".into(),
        ])
    }

    fn root_qdisc(&self) -> TcCommand {
        self.command(vec![
            "qdisc".into(),
            "add".into(),
            "dev".into(),
            self.interface.clone(),
            "root".into(),
            "handle".into(),
            format!("{PILOT_QDISC_HANDLE}:"),
            "prio".into(),
            "bands".into(),
            "3".into(),
        ])
    }

    fn pilot_filter(&self, injection: &Injection) -> Result<TcCommand> {
        let target = &injection.target;
        let mut arguments = vec![
            "filter".into(),
            "add".into(),
            "dev".into(),
            self.interface.clone(),
            "protocol".into(),
            "ip".into(),
            "parent".into(),
            format!("{PILOT_QDISC_HANDLE}:"),
            "prio".into(),
            "1".into(),
            "u32".into(),
        ];

        if let Some(source) = target.source {
            arguments.extend(match_ip_arg("src", source)?);
        }
        if let Some(destination) = target.destination {
            arguments.extend(match_ip_arg("dst", destination)?);
        }
        if let Some(port) = target.destination_port {
            arguments.extend(vec![
                "match".into(),
                "ip".into(),
                "dport".into(),
                port.to_string(),
                "0xffff".into(),
            ]);
        }

        arguments.extend(vec!["flowid".into(), format!("{PILOT_QDISC_HANDLE}:2")]);
        Ok(self.command(arguments))
    }

    fn netem_commands(&self, injection: &Injection) -> Vec<TcCommand> {
        let mut arguments = vec![
            "qdisc".into(),
            "add".into(),
            "dev".into(),
            self.interface.clone(),
            "parent".into(),
            format!("{PILOT_QDISC_HANDLE}:2"),
            "handle".into(),
            "50:".into(),
            "netem".into(),
        ];

        match &injection.fault {
            FaultSpec::Latency(spec) => {
                arguments.push("delay".into());
                arguments.push(format!("{}ms", spec.milliseconds));
            }
            FaultSpec::Loss(spec) => {
                arguments.push("loss".into());
                arguments.push(format_percent(spec.percent));
            }
        }
        vec![self.command(arguments)]
    }

    fn default_qdisc(&self) -> TcCommand {
        self.command(vec![
            "qdisc".into(),
            "add".into(),
            "dev".into(),
            self.interface.clone(),
            "parent".into(),
            format!("{PILOT_QDISC_HANDLE}:3"),
            "fq_codel".into(),
        ])
    }

    fn default_filter(&self) -> TcCommand {
        self.command(vec![
            "filter".into(),
            "add".into(),
            "dev".into(),
            self.interface.clone(),
            "protocol".into(),
            "ip".into(),
            "parent".into(),
            format!("{PILOT_QDISC_HANDLE}:"),
            "prio".into(),
            "2".into(),
            "u32".into(),
            "match".into(),
            "u32".into(),
            "0".into(),
            "0".into(),
            "flowid".into(),
            format!("{PILOT_QDISC_HANDLE}:3"),
        ])
    }
}

fn validate(injection: &Injection) -> Result<()> {
    if let FaultSpec::Loss(ref spec) = injection.fault {
        if !(0.0..=100.0).contains(&spec.percent) || spec.percent == 0.0 {
            return Err(AdapterError::InvalidLossPercent {
                percent: spec.percent,
            });
        }
    }

    match injection.target.source.or(injection.target.destination) {
        Some(IpNet::V6(_)) => Err(AdapterError::UnsupportedIpv6),
        Some(_) => Ok(()),
        None => Err(AdapterError::EmptyTarget),
    }
}

fn match_ip_arg(field: &str, network: IpNet) -> Result<Vec<String>> {
    match network {
        IpNet::V4(network) => Ok(vec![
            "match".into(),
            "ip".into(),
            field.into(),
            network.to_string(),
        ]),
        IpNet::V6(_) => Err(AdapterError::UnsupportedIpv6),
    }
}

fn format_percent(percent: f64) -> String {
    format!("{percent:.3}")
}

pub fn default_device() -> anyhow::Result<String> {
    let routes = std::fs::read_to_string("/proc/net/route")?;
    let mut best: Option<(u32, String)> = None;
    for line in routes.lines().skip(1) {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 4 {
            continue;
        }

        let Ok(metric) = columns[3].parse::<u32>() else {
            continue;
        };
        let interface = columns[0].to_string();
        match &best {
            Some((best_metric, _)) if metric >= *best_metric => {}
            _ => best = Some((metric, interface)),
        }
    }

    best.map(|(_, interface)| interface)
        .ok_or_else(|| anyhow::anyhow!("no IPv4 route found in /proc/net/route"))
}

#[allow(dead_code)]
fn _type_assertions(_: Ipv4Addr, _: Ipv6Addr, _: Ipv4Net) {}
