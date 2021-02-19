use crate::{
    db::DbConnection,
    error::Result,
    models::{AceAction, AceProtocol, AclDirection, AclType, Device},
    services::config_service::{get_config_value, set_config_value},
};
use namib_shared::config_firewall::{
    EnTarget, FirewallDevice, FirewallRule, NetworkConfig, NetworkHost, Protocol, ResolvedIp, RuleName,
};
use std::{
    net::{IpAddr, ToSocketAddrs},
    sync::atomic::{AtomicU32, Ordering},
};

static VERSION: AtomicU32 = AtomicU32::new(0);

pub fn convert_device_to_fw_rules(device: &Device) -> Result<FirewallDevice> {
    let mut index = 0;
    let mut result: Vec<FirewallRule> = Vec::new();
    let mud_data = match &device.mud_data {
        Some(mud_data) => mud_data,
        None => return Ok(FirewallDevice::new(device.id, device.ip_addr, result)),
    };

    for acl in &mud_data.acllist {
        for ace in &acl.ace {
            let rule_name = RuleName::new(format!("rule_{}", index));
            let protocol = match &ace.matches.protocol {
                None => Protocol::All,
                Some(proto) => match proto {
                    AceProtocol::TCP => Protocol::Tcp,
                    AceProtocol::UDP => Protocol::Udp,
                    AceProtocol::Protocol(_proto_nr) => Protocol::All, // Default to all protocols if protocol is not supported.
                                                                       // TODO add support for more protocols
                },
            };
            let target = match ace.action {
                AceAction::Accept => EnTarget::ACCEPT,
                AceAction::Deny => EnTarget::REJECT,
            };

            if let Some(dns_name) = &ace.matches.dnsname {
                let route_network_lan;
                let route_network_wan;
                if let Ok(addr) = dns_name.parse::<IpAddr>() {
                    route_network_lan = NetworkConfig::new(Some(NetworkHost::FirewallDevice), None);
                    route_network_wan = NetworkConfig::new(Some(NetworkHost::Ip(addr)), None);
                } else {
                    route_network_lan = NetworkConfig::new(Some(NetworkHost::FirewallDevice), None);
                    route_network_wan = NetworkConfig::new(
                        Some(NetworkHost::Hostname {
                            dns_name: dns_name.clone(),
                            resolved_ip: ResolvedIp::default(),
                        }),
                        None,
                    );
                }

                let (route_network_src, route_network_dest) = match acl.packet_direction {
                    AclDirection::FromDevice => (route_network_lan, route_network_wan),
                    AclDirection::ToDevice => (route_network_wan, route_network_lan),
                };
                let config_firewall = FirewallRule::new(
                    rule_name.clone(),
                    route_network_src,
                    route_network_dest,
                    protocol.clone(),
                    target.clone(),
                );
                result.push(config_firewall);
            } else {
                //let route_network_lan = NetworkConfig::new(None, None);
                //let route_network_wan = NetworkConfig::new(None, None);
                //let (route_network_src, route_network_dest) = match acl.packet_direction {
                //    AclDirection::FromDevice => (route_network_lan, route_network_wan),
                //    AclDirection::ToDevice => (route_network_wan, route_network_lan),
                //};
                //let config_firewall =
                //    FirewallRule::new(rule_name, route_network_src, route_network_dest, protocol, target);
                //result.push(config_firewall);
            }
            index += 1;
        }
    }
    result.push(FirewallRule::new(
        RuleName::new(format!("rule_default_{}", index)),
        NetworkConfig::new(Some(NetworkHost::FirewallDevice), None),
        NetworkConfig::new(None, None),
        Protocol::All,
        EnTarget::REJECT,
    ));
    index += 1;
    result.push(FirewallRule::new(
        RuleName::new(format!("rule_default_{}", index)),
        NetworkConfig::new(None, None),
        NetworkConfig::new(Some(NetworkHost::FirewallDevice), None),
        Protocol::All,
        EnTarget::REJECT,
    ));

    Ok(FirewallDevice::new(device.id, device.ip_addr, result))
}

pub async fn get_config_version(pool: &DbConnection) -> String {
    get_config_value("version".to_string(), pool)
        .await
        .unwrap_or_else(|_| "0".to_string())
}

pub async fn update_config_version(pool: &DbConnection) {
    set_config_value(
        "version".to_string(),
        (get_config_value("version".to_string(), pool)
            .await
            .unwrap_or_else(|_| "0".to_string())
            .parse::<u32>()
            .unwrap_or(1)
            + 1)
        .to_string(),
        pool,
    )
    .await
    .expect("failed to write config");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Ace, AceAction, AceMatches, AceProtocol, Acl, AclDirection, AclType, Device, MudData};
    use chrono::Local;
    use namib_shared::mac;

    #[test]
    fn test_converting() -> Result<()> {
        let mud_data = MudData {
            url: "example.com/.well-known/mud".to_string(),
            masa_url: None,
            last_update: "some_last_update".to_string(),
            systeminfo: Some("some_systeminfo".to_string()),
            mfg_name: Some("some_mfg_name".to_string()),
            model_name: Some("some_model_name".to_string()),
            documentation: Some("some_documentation".to_string()),
            expiration: Local::now(),
            acllist: vec![Acl {
                name: "some_acl_name".to_string(),
                packet_direction: AclDirection::ToDevice,
                acl_type: AclType::IPV6,
                ace: vec![Ace {
                    name: "some_ace_name".to_string(),
                    action: AceAction::Accept,
                    matches: AceMatches {
                        protocol: Some(AceProtocol::TCP),
                        direction_initiated: None,
                        address_mask: None,
                        dnsname: None,
                        source_port: None,
                        destination_port: None,
                    },
                }],
            }],
        };

        let device = Device {
            id: 0,
            mac_addr: Some("aa:bb:cc:dd:ee:ff".parse::<mac::MacAddr>().unwrap().into()),
            ip_addr: "127.0.0.1".parse().unwrap(),
            hostname: "".to_string(),
            vendor_class: "".to_string(),
            mud_url: Some("http://example.com/mud_url.json".to_string()),
            mud_data: Some(mud_data),
            last_interaction: Local::now().naive_local(),
        };

        let x = convert_device_to_fw_rules(&device)?;

        println!("{:#?}", x);

        let opts = x[0].to_option();
        assert!(opts.iter().any(|x| x.0 == "name" && x.1 == "rule_0"));
        assert!(opts.iter().any(|x| x.0 == "src" && x.1 == "wan"));
        assert!(opts.iter().any(|x| x.0 == "dest" && x.1 == "lan"));
        assert!(opts.iter().any(|x| x.0 == "dest_ip" && x.1 == "127.0.0.1"));
        assert!(opts.iter().any(|x| x.0 == "proto" && x.1 == "6"));
        assert!(opts.iter().any(|x| x.0 == "target" && x.1 == "ACCEPT"));

        Ok(())
    }
}
