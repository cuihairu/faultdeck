use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencySpec {
    pub milliseconds: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LossSpec {
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisconnectSpec {
    pub all_traffic: bool,
}

impl Default for DisconnectSpec {
    fn default() -> Self {
        Self { all_traffic: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FaultSpec {
    Latency(LatencySpec),
    Loss(LossSpec),
    Disconnect(DisconnectSpec),
}

impl FaultSpec {
    pub fn kind_name(&self) -> &'static str {
        match self {
            FaultSpec::Latency(_) => "latency",
            FaultSpec::Loss(_) => "loss",
            FaultSpec::Disconnect(_) => "disconnect",
        }
    }
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

    /// Iterate all address-family-bearing fields.
    pub fn networks(&self) -> impl Iterator<Item = &IpNet> {
        self.source.iter().chain(self.destination.iter())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Injection {
    pub fault: FaultSpec,
    pub target: Target,
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("fault target is empty")]
    EmptyTarget,
    #[error("invalid loss percent {percent}; expected 0 < percent <= 100")]
    InvalidLossPercent { percent: f64 },
    #[error("invalid latency {milliseconds}ms; expected 0 < milliseconds <= {max}")]
    InvalidLatency { milliseconds: u64, max: u64 },
    #[error("IPv6 target is unsupported by the current adapter")]
    UnsupportedIpv6,
    #[error(
        "target {target} overlaps management network {management}; management traffic must never be faulted"
    )]
    OverlapsManagement { target: String, management: String },
}

pub const MAX_LATENCY_MS: u64 = 86_400_000; // 24h

/// Validate an injection against intrinsic constraints (families, ranges).
/// Management-network allowlist is enforced separately by the agent guard.
pub fn validate_injection(injection: &Injection) -> Result<(), CoreError> {
    match &injection.fault {
        FaultSpec::Loss(spec) => {
            if !(0.0..=100.0).contains(&spec.percent) || spec.percent == 0.0 {
                return Err(CoreError::InvalidLossPercent {
                    percent: spec.percent,
                });
            }
        }
        FaultSpec::Latency(spec) => {
            if spec.milliseconds == 0 || spec.milliseconds > MAX_LATENCY_MS {
                return Err(CoreError::InvalidLatency {
                    milliseconds: spec.milliseconds,
                    max: MAX_LATENCY_MS,
                });
            }
        }
        FaultSpec::Disconnect(_) => {}
    }

    let mut seen = false;
    for network in injection.target.networks() {
        seen = true;
        if let IpNet::V6(_) = network {
            return Err(CoreError::UnsupportedIpv6);
        }
    }
    if !seen {
        return Err(CoreError::EmptyTarget);
    }
    Ok(())
}

/// True if `network` overlaps any of the protected management networks.
pub fn overlaps_management(network: &IpNet, management: &[IpNet]) -> bool {
    management.iter().any(|m| networks_overlap(network, m))
}

pub fn networks_overlap(a: &IpNet, b: &IpNet) -> bool {
    match (a, b) {
        (IpNet::V4(x), IpNet::V4(y)) => x.contains(&y.addr()) || y.contains(&x.addr()),
        (IpNet::V6(x), IpNet::V6(y)) => x.contains(&y.addr()) || y.contains(&x.addr()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn latency(ms: u64) -> Injection {
        Injection {
            fault: FaultSpec::Latency(LatencySpec { milliseconds: ms }),
            target: Target::destination_port(IpNet::from_str("192.0.2.0/24").unwrap(), 7000),
        }
    }

    #[test]
    fn rejects_mixed_family() {
        let mixed = Injection {
            fault: FaultSpec::Loss(LossSpec { percent: 1.0 }),
            target: Target {
                source: Some(IpNet::from_str("192.0.2.0/24").unwrap()),
                destination: Some(IpNet::from_str("2001:db8::/32").unwrap()),
                destination_port: None,
            },
        };
        assert!(matches!(
            validate_injection(&mixed),
            Err(CoreError::UnsupportedIpv6)
        ));
    }

    #[test]
    fn rejects_bad_ranges() {
        assert!(matches!(
            validate_injection(&latency(0)),
            Err(CoreError::InvalidLatency { .. })
        ));
        assert!(matches!(
            validate_injection(&latency(u64::MAX)),
            Err(CoreError::InvalidLatency { .. })
        ));
    }

    #[test]
    fn detects_management_overlap() {
        let target = IpNet::from_str("10.0.0.0/8").unwrap();
        let mgmt = [IpNet::from_str("10.1.0.0/16").unwrap()];
        assert!(overlaps_management(&target, &mgmt));
        assert!(!overlaps_management(&target, &[]));
    }
}
