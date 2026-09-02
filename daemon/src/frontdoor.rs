//! An address of the unit's own where `rayhunter.local` is Rayhunter.
//!
//! The hotspot's admin pages own ports 80 and 443 on the hotspot address,
//! so a name that resolves there and is typed without a port lands on the
//! hotspot, not on Rayhunter. That is what happened the first time anyone
//! tried it. The fix is a second address on the hotspot interface, above
//! the DHCP pool, that belongs to Rayhunter alone: `rayhunter.local` resolves
//! to it, and its ports 80 and 443 are steered to the daemon's own ports by
//! a NAT rule, since the admin server's wildcard socket refuses to share
//! those ports even on another address.
//!
//! The daemon has the capabilities for this (the adb shell does not, on an
//! Orbic, which is why it is done here and not by the installer). Every
//! step is checked before it is repeated, so a restart does not stack rules,
//! and undone on the way out. Any failure leaves the name pointing at the
//! hotspot address as before, where the port still works.

use std::net::Ipv4Addr;

use log::{info, warn};
use tokio::process::Command;

/// The host octet of the alias, at the top of the hotspot's /24. The DHCP
/// pools on every supported hotspot stop well short of it (.100 to .200 on
/// the Qualcomm mobileap units), and .255 is the broadcast address.
pub const ALIAS_HOST: u8 = 254;

/// The address Rayhunter takes beside a hotspot address.
pub fn alias_for(hotspot: Ipv4Addr) -> Ipv4Addr {
    let [a, b, c, d] = hotspot.octets();
    // A hotspot already at .254 gets the next one down.
    let host = if d == ALIAS_HOST {
        ALIAS_HOST - 1
    } else {
        ALIAS_HOST
    };
    Ipv4Addr::new(a, b, c, host)
}

/// The NAT rules, as `iptables` arguments after the table and chain, that
/// send the alias's well-known ports to the daemon's.
fn rules(iface: &str, alias: Ipv4Addr, port: u16, tls_port: u16) -> Vec<Vec<String>> {
    [(443u16, tls_port), (80u16, port)]
        .iter()
        .map(|(from, to)| {
            [
                "-i",
                iface,
                "-d",
                &alias.to_string(),
                "-p",
                "tcp",
                "--dport",
                &from.to_string(),
                "-j",
                "REDIRECT",
                "--to-ports",
                &to.to_string(),
            ]
            .iter()
            .map(|s| s.to_string())
            .collect()
        })
        .collect()
}

async fn run(program: &str, args: &[String]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("{program}: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "{program} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// The alias and rules in place, to be taken down on drop of the daemon.
pub struct FrontDoor {
    pub iface: String,
    pub alias: Ipv4Addr,
    rules: Vec<Vec<String>>,
}

impl FrontDoor {
    /// Add the alias beside `hotspot` on `iface` and steer its ports.
    ///
    /// `None`, with the reason logged, if any of it fails; the caller then
    /// carries on with the hotspot address alone.
    pub async fn open(iface: &str, hotspot: Ipv4Addr, port: u16, tls_port: u16) -> Option<Self> {
        let alias = alias_for(hotspot);
        let cidr = format!("{alias}/24");
        match run("ip", &args(&["addr", "add", &cidr, "dev", iface])).await {
            Ok(_) => {}
            // Already there from a previous run that did not get to clean up.
            Err(e) if e.contains("File exists") => {}
            Err(e) => {
                warn!("no second address for rayhunter.local: {e}");
                return None;
            }
        }
        let rules = rules(iface, alias, port, tls_port);
        for rule in &rules {
            let mut check = args(&["-t", "nat", "-C", "PREROUTING"]);
            check.extend(rule.iter().cloned());
            if run("iptables", &check).await.is_ok() {
                continue;
            }
            let mut add = args(&["-t", "nat", "-A", "PREROUTING"]);
            add.extend(rule.iter().cloned());
            if let Err(e) = run("iptables", &add).await {
                warn!("could not steer {alias} to the daemon: {e}");
                let door = Self {
                    iface: iface.to_string(),
                    alias,
                    rules: rules.clone(),
                };
                door.close().await;
                return None;
            }
        }
        info!("rayhunter.local is {alias} on {iface}; its ports 80 and 443 reach the daemon");
        Some(Self {
            iface: iface.to_string(),
            alias,
            rules,
        })
    }

    /// Remove the rules and the alias.
    pub async fn close(&self) {
        for rule in &self.rules {
            let mut del = args(&["-t", "nat", "-D", "PREROUTING"]);
            del.extend(rule.iter().cloned());
            // Absent is as good as removed.
            let _ = run("iptables", &del).await;
        }
        let cidr = format!("{}/24", self.alias);
        let _ = run("ip", &args(&["addr", "del", &cidr, "dev", &self.iface])).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_alias_sits_at_the_top_of_the_hotspots_network() {
        assert_eq!(
            alias_for(Ipv4Addr::new(192, 168, 1, 1)),
            Ipv4Addr::new(192, 168, 1, 254)
        );
        assert_eq!(
            alias_for(Ipv4Addr::new(192, 168, 0, 1)),
            Ipv4Addr::new(192, 168, 0, 254)
        );
        assert_eq!(
            alias_for(Ipv4Addr::new(10, 0, 0, 254)),
            Ipv4Addr::new(10, 0, 0, 253)
        );
    }

    #[test]
    fn the_rules_steer_both_ports_on_the_alias_only() {
        let r = rules("bridge0", Ipv4Addr::new(192, 168, 1, 254), 8080, 8443);
        assert_eq!(r.len(), 2);
        assert_eq!(
            r[0].join(" "),
            "-i bridge0 -d 192.168.1.254 -p tcp --dport 443 -j REDIRECT --to-ports 8443"
        );
        assert_eq!(
            r[1].join(" "),
            "-i bridge0 -d 192.168.1.254 -p tcp --dport 80 -j REDIRECT --to-ports 8080"
        );
    }
}
