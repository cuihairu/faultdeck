use std::fmt;
use std::path::PathBuf;

use faultdeck_core::{
    validate_injection as core_validate, CoreError, FaultSpec as CoreFaultSpec,
    Injection as CoreInjection, LatencySpec as CoreLatencySpec, LossSpec as CoreLossSpec,
    Target as CoreTarget,
};
use ipnet::IpNet;
use thiserror::Error;

pub use faultdeck_core::MAX_LATENCY_MS;
pub const PILOT_QDISC_HANDLE: u32 = 100;
const DEFAULT_ROUTE_DESTINATION: &str = "00000000";
const ROUTE_FLAG_UP: u32 = 0x0001;

// Re-export shared model types under netem namespace.
pub type LatencySpec = CoreLatencySpec;
pub type LossSpec = CoreLossSpec;
pub type FaultSpec = CoreFaultSpec;
pub type Target = CoreTarget;
pub type Injection = CoreInjection;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("tc command failed: {program} {arguments}: {message}")]
    Command {
        program: String,
        arguments: String,
        message: String,
    },
    #[error("tc binary missing: {program}")]
    ProgramMissing { program: String },
    #[error("fault target is empty")]
    EmptyTarget,
    #[error("invalid loss percent {percent}; expected 0 < percent <= 100")]
    InvalidLossPercent { percent: f64 },
    #[error("invalid latency {milliseconds}ms; expected 0 < milliseconds <= {MAX_LATENCY_MS}")]
    InvalidLatency { milliseconds: u64 },
    #[error("target overlaps a management network; management traffic must never be faulted")]
    ManagementOverlap,
    #[error("IPv6 target is unsupported by the current tc adapter")]
    UnsupportedIpv6,
    #[error(
        "interface {interface} already has root qdisc '{kind}' (handle {handle}); refusing to overwrite"
    )]
    ExistingRootQdisc {
        interface: String,
        kind: String,
        handle: String,
    },
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
    fn run_capture(&self, _command: &TcCommand) -> Result<String> {
        Ok(String::new())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemCommandExecutor {
    pub dry_run: bool,
}

impl SystemCommandExecutor {
    fn spawn(
        &self,
        command: &TcCommand,
    ) -> std::result::Result<std::process::Output, std::io::Error> {
        std::process::Command::new(&command.program)
            .args(&command.arguments)
            .output()
    }
}

impl CommandExecutor for SystemCommandExecutor {
    #[cfg(target_os = "linux")]
    fn run(&self, command: &TcCommand) -> Result<()> {
        if self.dry_run {
            return Ok(());
        }
        let output = self
            .spawn(command)
            .map_err(|error| map_spawn_error(command, error))?;
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

    #[cfg(target_os = "linux")]
    fn run_capture(&self, command: &TcCommand) -> Result<String> {
        if self.dry_run {
            return Ok(String::new());
        }
        let output = self
            .spawn(command)
            .map_err(|error| map_spawn_error(command, error))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(AdapterError::Command {
                program: command.program.display().to_string(),
                arguments: command.arguments.join(" "),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }
}

fn map_spawn_error(command: &TcCommand, error: std::io::Error) -> AdapterError {
    if error.kind() == std::io::ErrorKind::NotFound {
        AdapterError::ProgramMissing {
            program: command.program.display().to_string(),
        }
    } else {
        AdapterError::Command {
            program: command.program.display().to_string(),
            arguments: command.arguments.join(" "),
            message: error.to_string(),
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
        Ok(self.apply_commands(injection))
    }

    pub fn inject(&self, injection: &Injection) -> Result<Vec<TcCommand>> {
        validate(injection)?;
        let leftover = self.preflight()?;
        let commands = self.apply_commands(injection);
        // own_qdisc_created must only become true once our own 100: root
        // qdisc has actually been created. Marking the pre-clean del as our
        // qdisc would let a subsequent failed `add` delete the foreign qdisc
        // that appeared in the TOCTOU window.
        let mut own_qdisc_created = false;
        if leftover {
            if let Err(error) = self.executor.run(&self.rollback_command()) {
                return Err(error);
            }
        }
        for command in &commands {
            if let Err(error) = self.executor.run(command) {
                // Only clean up a root qdisc we actually created. If the very
                // first (root_qdisc) command failed because a foreign qdisc
                // appeared in the TOCTOU window, we must NOT delete it.
                if own_qdisc_created {
                    let _ = self.rollback_cleanup();
                }
                return Err(error);
            }
            // The first executed command after cleanup is the root qdisc add.
            own_qdisc_created = true;
        }
        Ok(commands)
    }

    pub fn rollback(&self) -> Result<Vec<TcCommand>> {
        let output = match self.executor.run_capture(&self.show_command()) {
            Ok(output) => output,
            Err(AdapterError::Command { message, .. }) if is_tc_not_found_message(&message) => {
                // Interface missing / no qdisc: nothing to inspect. The del
                // below is itself tolerant, so rollback stays idempotent.
                String::new()
            }
            Err(error) => return Err(error),
        };
        if let Some((kind, handle)) = parse_root_qdisc(&output) {
            if !handle.starts_with(&format!("{PILOT_QDISC_HANDLE}:")) {
                return Err(AdapterError::ExistingRootQdisc {
                    interface: self.interface.clone(),
                    kind,
                    handle,
                });
            }
        }
        let command = self.rollback_command();
        match self.executor.run(&command) {
            Ok(()) => {}
            Err(AdapterError::Command { message, .. }) if is_tc_not_found_message(&message) => {}
            Err(error) => return Err(error),
        }
        Ok(vec![command])
    }

    fn preflight(&self) -> Result<bool> {
        let output = self.executor.run_capture(&self.show_command())?;
        if let Some((kind, handle)) = parse_root_qdisc(&output) {
            if handle.starts_with(&format!("{PILOT_QDISC_HANDLE}:")) {
                // A leftover of our own qdisc: caller must clean before adding.
                return Ok(true);
            } else {
                return Err(AdapterError::ExistingRootQdisc {
                    interface: self.interface.clone(),
                    kind,
                    handle,
                });
            }
        }
        Ok(false)
    }

    fn rollback_cleanup(&self) -> Result<()> {
        let command = self.rollback_command();
        match self.executor.run(&command) {
            Ok(()) => Ok(()),
            Err(AdapterError::Command { message, .. }) if is_tc_not_found_message(&message) => {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn apply_commands(&self, injection: &Injection) -> Vec<TcCommand> {
        let mut commands = vec![self.root_qdisc()];
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

    fn show_command(&self) -> TcCommand {
        self.command(vec![
            "qdisc".into(),
            "show".into(),
            "dev".into(),
            self.interface.clone(),
        ])
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
            CoreFaultSpec::Latency(spec) => {
                arguments.push("delay".into());
                arguments.push(format!("{}ms", spec.milliseconds));
            }
            CoreFaultSpec::Loss(spec) => {
                arguments.push("loss".into());
                arguments.push(format_percent(spec.percent));
            }
            CoreFaultSpec::Disconnect(_) => {
                // Disconnect is handled by iptables in the agent; netem has no
                // equivalent. Reaching this path is a programming error.
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
    match core_validate(injection) {
        Ok(()) => Ok(()),
        Err(CoreError::EmptyTarget) => Err(AdapterError::EmptyTarget),
        Err(CoreError::InvalidLossPercent { percent }) => {
            Err(AdapterError::InvalidLossPercent { percent })
        }
        Err(CoreError::InvalidLatency { milliseconds, .. }) => {
            Err(AdapterError::InvalidLatency { milliseconds })
        }
        Err(CoreError::UnsupportedIpv6) => Err(AdapterError::UnsupportedIpv6),
        Err(CoreError::OverlapsManagement { .. }) => Err(AdapterError::ManagementOverlap),
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

fn is_tc_not_found_message(message: &str) -> bool {
    message.contains("Cannot find")
        || message.contains("Cannot delete")
        || message.contains("No such file")
        || message.contains("No such device")
}

fn parse_root_qdisc(output: &str) -> Option<(String, String)> {
    for line in output.lines() {
        let mut tokens = line.split_whitespace();
        if tokens.next() != Some("qdisc") {
            continue;
        }
        let (Some(kind), Some(handle), rest) = (tokens.next(), tokens.next(), tokens) else {
            // Truncated line: skip instead of aborting so later valid lines
            // (e.g. a foreign root qdisc) are still detected.
            continue;
        };
        let (kind, handle) = (kind.to_string(), handle.to_string());
        let mut tokens = rest;
        if tokens.any(|t| t == "root") {
            return Some((kind, handle));
        }
    }
    None
}

pub fn default_device() -> anyhow::Result<String> {
    let routes = std::fs::read_to_string("/proc/net/route")?;
    default_device_from_routes(&routes)
}

fn default_device_from_routes(routes: &str) -> anyhow::Result<String> {
    let mut best: Option<(u32, String)> = None;
    for line in routes.lines().skip(1) {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 8 {
            continue;
        }
        if columns[1] != DEFAULT_ROUTE_DESTINATION {
            continue;
        }
        let Ok(flags) = u32::from_str_radix(columns[3], 16) else {
            continue;
        };
        if flags & ROUTE_FLAG_UP == 0 {
            continue;
        }
        let Ok(metric) = columns[6].parse::<u32>() else {
            continue;
        };
        let interface = columns[0].to_string();
        match &best {
            Some((best_metric, _)) if metric >= *best_metric => {}
            _ => best = Some((metric, interface)),
        }
    }

    best.map(|(_, interface)| interface)
        .ok_or_else(|| anyhow::anyhow!("no default IPv4 route found in /proc/net/route"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn latency(ms: u64) -> Injection {
        Injection {
            fault: CoreFaultSpec::Latency(CoreLatencySpec { milliseconds: ms }),
            target: CoreTarget::destination_port(IpNet::from_str("192.0.2.0/24").unwrap(), 7000),
        }
    }

    #[test]
    fn rejects_mixed_family_target() {
        let mixed = Injection {
            fault: CoreFaultSpec::Loss(CoreLossSpec { percent: 1.0 }),
            target: CoreTarget {
                source: Some(IpNet::from_str("192.0.2.0/24").unwrap()),
                destination: Some(IpNet::from_str("2001:db8::/32").unwrap()),
                destination_port: None,
            },
        };
        assert!(matches!(
            validate(&mixed),
            Err(AdapterError::UnsupportedIpv6)
        ));
    }

    #[test]
    fn rejects_latency_zero_and_overflow() {
        assert!(matches!(
            validate(&latency(0)),
            Err(AdapterError::InvalidLatency { .. })
        ));
        assert!(matches!(
            validate(&latency(u64::MAX)),
            Err(AdapterError::InvalidLatency { .. })
        ));
        assert!(matches!(
            validate(&latency(MAX_LATENCY_MS + 1)),
            Err(AdapterError::InvalidLatency { .. })
        ));
        assert!(validate(&latency(1)).is_ok());
    }

    #[test]
    fn parses_default_route_metric() {
        let routes = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
eth0\t00000000\t01020304\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
eth1\t00000000\t01020305\t0001\t0\t0\t50\t00000000\t0\t0\t0\n\
vpn0\t00000000\t01020306\t0001\t0\t0\t200\t00000000\t0\t0\t0\n\
eth2\t000A0000\t00000000\t0001\t0\t0\t10\t0000FFFF\t0\t0\t0\n";
        assert_eq!(default_device_from_routes(routes).unwrap(), "eth1");
    }

    #[test]
    fn ignores_down_and_nondefault_routes() {
        let routes = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
eth0\t00000000\t01020304\t0000\t0\t0\t1\t00000000\t0\t0\t0\n\
eth1\t000A0000\t00000000\t0001\t0\t0\t1\t0000FFFF\t0\t0\t0\n";
        assert!(default_device_from_routes(routes).is_err());
    }

    #[test]
    fn rollback_suppresses_only_tc_not_found() {
        let adapter = NetemAdapter::with_executor("eth0", |_: &TcCommand| {
            Err(AdapterError::ProgramMissing {
                program: "tc".into(),
            })
        });
        let result = adapter.rollback();
        assert!(result.is_err());
    }

    #[test]
    fn parse_root_qdisc_detects_own_and_foreign() {
        let own = "qdisc prio 100: root refcnt 2 tc 100: prio bands 3\n";
        assert!(parse_root_qdisc(own).unwrap().1.starts_with("100:"));
        let foreign = "qdisc fq_codel 1: root refcnt 2\n";
        assert_eq!(parse_root_qdisc(foreign).unwrap().0, "fq_codel");
        assert!(parse_root_qdisc("").is_none());
    }

    #[test]
    fn parse_root_qdisc_skips_truncated_line() {
        let output = "qdisc prio\nqdisc fq_codel 1: root refcnt 2\n";
        assert_eq!(
            parse_root_qdisc(output).unwrap().0,
            "fq_codel",
            "truncated line must not abort scanning"
        );
    }

    #[test]
    fn management_overlap_error_is_distinct() {
        // Ensure the previously misleading EmptyTarget mapping is gone: an
        // overlap now surfaces its own, correctly-worded error.
        let err = AdapterError::ManagementOverlap;
        let msg = err.to_string();
        assert!(msg.contains("management"), "message was: {msg}");
        assert!(!msg.contains("fault target is empty"));
    }
}
