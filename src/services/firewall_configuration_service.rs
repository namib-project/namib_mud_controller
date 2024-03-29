// Copyright 2020-2021, Benjamin Ludewig, Florian Bonetti, Jeffrey Munstermann, Luca Nittscher, Hugo Damer, Michael Bach
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::net::IpAddr;

use namib_shared::{
    firewall_config::{FirewallDevice, FirewallRule, Protocol, RuleName, RuleTarget, RuleTargetHost, Verdict},
    EnforcerConfig,
};

use crate::{
    db::DbConnection,
    error::Result,
    models::{AceAction, AceProtocol, Acl, AclDirection, DeviceWithRefs},
    services::{
        acme_service,
        config_service::{get_config_value, set_config_value, ConfigKeys},
    },
};

pub fn merge_acls<'a>(original: &'a [Acl], override_with: &'a [Acl]) -> Vec<&'a Acl> {
    let override_keys: Vec<&str> = override_with.iter().map(|x| x.name.as_ref()).collect();
    original
        .iter()
        .filter(|x| !override_keys.contains(&x.name.as_str()))
        .chain(override_with.iter())
        .collect()
}

pub fn create_configuration(version: String, devices: &[DeviceWithRefs]) -> EnforcerConfig {
    let rules = devices
        .iter()
        .filter(|d| d.ipv4_addr.is_some() || d.ipv6_addr.is_some())
        .map(|d| convert_device_to_fw_rules(d))
        .collect();
    EnforcerConfig::new(version, rules, acme_service::DOMAIN.clone())
}

pub fn convert_device_to_fw_rules(device: &DeviceWithRefs) -> FirewallDevice {
    let mut index = 0;
    let mut result: Vec<FirewallRule> = Vec::new();
    let mud_data = match &device.mud_data {
        Some(mud_data) => mud_data,
        None => {
            return FirewallDevice {
                id: device.id,
                ipv4_addr: device.ipv4_addr,
                ipv6_addr: device.ipv6_addr,
                rules: result,
                collect_data: device.collect_info,
            }
        },
    };

    let merged_acls = if mud_data.acl_override.is_empty() {
        mud_data.acllist.iter().collect()
    } else {
        merge_acls(&mud_data.acllist, &mud_data.acl_override)
    };

    for acl in &merged_acls {
        for ace in &acl.ace {
            let rule_name = RuleName::new(format!("rule_{}", index));
            let protocol = match &ace.matches.protocol {
                None => Protocol::All,
                Some(proto) => match proto {
                    AceProtocol::Tcp => Protocol::Tcp,
                    AceProtocol::Udp => Protocol::Udp,
                    AceProtocol::Protocol(_proto_nr) => Protocol::All, // Default to all protocols if protocol is not supported.
                                                                       // TODO add support for more protocols
                },
            };
            let target = match ace.action {
                AceAction::Accept => Verdict::Accept,
                AceAction::Deny => Verdict::Reject,
            };

            let route_network_fw_device = RuleTarget::new(Some(RuleTargetHost::FirewallDevice), None);
            if let Some(dns_name) = &ace.matches.dnsname {
                let route_network_remote_host = match dns_name.parse::<IpAddr>() {
                    Ok(addr) => RuleTarget::new(Some(RuleTargetHost::Ip(addr)), None),
                    Err(_) => RuleTarget::new(Some(RuleTargetHost::Hostname(dns_name.clone())), None),
                };

                let (route_network_src, route_network_dest) = match acl.packet_direction {
                    AclDirection::FromDevice => (route_network_fw_device, route_network_remote_host),
                    AclDirection::ToDevice => (route_network_remote_host, route_network_fw_device),
                };
                let config_firewall = FirewallRule::new(
                    rule_name.clone(),
                    route_network_src,
                    route_network_dest,
                    protocol.clone(),
                    target.clone(),
                );
                result.push(config_firewall);
            }
            index += 1;
        }
    }
    result.push(FirewallRule::new(
        RuleName::new(format!("rule_default_{}", index)),
        RuleTarget::new(Some(RuleTargetHost::FirewallDevice), None),
        RuleTarget::new(None, None),
        Protocol::All,
        Verdict::Reject,
    ));
    index += 1;
    result.push(FirewallRule::new(
        RuleName::new(format!("rule_default_{}", index)),
        RuleTarget::new(None, None),
        RuleTarget::new(Some(RuleTargetHost::FirewallDevice), None),
        Protocol::All,
        Verdict::Reject,
    ));

    FirewallDevice {
        id: device.id,
        ipv4_addr: device.ipv4_addr,
        ipv6_addr: device.ipv6_addr,
        rules: result,
        collect_data: device.collect_info,
    }
}

pub async fn get_config_version(pool: &DbConnection) -> String {
    get_config_value(ConfigKeys::FirewallConfigVersion.as_ref(), pool)
        .await
        .unwrap_or_else(|_| "0".to_string())
}

pub async fn update_config_version(pool: &DbConnection) -> Result<()> {
    let old_config_version = get_config_value(ConfigKeys::FirewallConfigVersion.as_ref(), pool)
        .await
        .unwrap_or(0u64);
    set_config_value(
        ConfigKeys::FirewallConfigVersion.as_ref(),
        old_config_version.wrapping_add(1),
        pool,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use namib_shared::macaddr::MacAddr;

    use super::*;
    use crate::models::{Ace, AceAction, AceMatches, AceProtocol, Acl, AclDirection, AclType, Device, MudData};

    #[test]
    fn test_acl_merging() -> Result<()> {
        let original_acls = vec![
            Acl {
                name: "acl_to_device".to_string(),
                packet_direction: AclDirection::ToDevice,
                acl_type: AclType::IPV6,
                ace: vec![Ace {
                    name: "acl_to_device_0".to_string(),
                    action: AceAction::Accept,
                    matches: AceMatches {
                        protocol: Some(AceProtocol::Tcp),
                        direction_initiated: None,
                        address_mask: None,
                        dnsname: None,
                        source_port: None,
                        destination_port: None,
                    },
                }],
            },
            Acl {
                name: "acl_from_device".to_string(),
                packet_direction: AclDirection::FromDevice,
                acl_type: AclType::IPV4,
                ace: vec![Ace {
                    name: "acl_from_device_0".to_string(),
                    action: AceAction::Deny,
                    matches: AceMatches {
                        protocol: Some(AceProtocol::Tcp),
                        direction_initiated: None,
                        address_mask: None,
                        dnsname: None,
                        source_port: None,
                        destination_port: None,
                    },
                }],
            },
        ];

        let override_acls = vec![
            Acl {
                name: "acl_to_device".to_string(),
                packet_direction: AclDirection::ToDevice,
                acl_type: AclType::IPV4,
                ace: vec![Ace {
                    name: "acl_to_device_0".to_string(),
                    action: AceAction::Accept,
                    matches: AceMatches {
                        protocol: Some(AceProtocol::Tcp),
                        direction_initiated: None,
                        address_mask: None,
                        dnsname: None,
                        source_port: None,
                        destination_port: None,
                    },
                }],
            },
            Acl {
                name: "acl_around_device_or_sth".to_string(),
                packet_direction: AclDirection::FromDevice,
                acl_type: AclType::IPV4,
                ace: vec![Ace {
                    name: "acl_around_device_or_sth_0".to_string(),
                    action: AceAction::Accept,
                    matches: AceMatches {
                        protocol: Some(AceProtocol::Udp),
                        direction_initiated: None,
                        address_mask: None,
                        dnsname: None,
                        source_port: None,
                        destination_port: None,
                    },
                }],
            },
        ];

        let merged_acls = merge_acls(&original_acls, &override_acls);

        let to_device_acl = merged_acls
            .iter()
            .find(|acl| acl.name == "acl_to_device")
            .expect("acl_to_device not in acls");
        assert_eq!(to_device_acl.ace, override_acls[0].ace);
        assert_eq!(to_device_acl.acl_type, override_acls[0].acl_type);
        assert_eq!(to_device_acl.packet_direction, override_acls[0].packet_direction);

        let from_device_acl = merged_acls
            .iter()
            .find(|acl| acl.name == "acl_from_device")
            .expect("acl_from_device not in acls");
        assert_eq!(from_device_acl.ace, original_acls[1].ace);
        assert_eq!(from_device_acl.acl_type, original_acls[1].acl_type);
        assert_eq!(from_device_acl.packet_direction, original_acls[1].packet_direction);

        let around_device_or_sth_acl = merged_acls
            .iter()
            .find(|acl| acl.name == "acl_around_device_or_sth")
            .expect("acl_around_device_or_sth not in acls");
        assert_eq!(around_device_or_sth_acl.ace, override_acls[1].ace);
        assert_eq!(around_device_or_sth_acl.acl_type, override_acls[1].acl_type);
        assert_eq!(
            around_device_or_sth_acl.packet_direction,
            override_acls[1].packet_direction
        );

        Ok(())
    }

    #[test]
    fn test_overridden_acls_to_firewall_rules() -> Result<()> {
        let mud_data = MudData {
            url: "example.com/.well-known/mud".to_string(),
            masa_url: None,
            last_update: "some_last_update".to_string(),
            systeminfo: Some("some_systeminfo".to_string()),
            mfg_name: Some("some_mfg_name".to_string()),
            model_name: Some("some_model_name".to_string()),
            documentation: Some("some_documentation".to_string()),
            expiration: Utc::now(),
            acllist: vec![Acl {
                name: "some_acl_name".to_string(),
                packet_direction: AclDirection::ToDevice,
                acl_type: AclType::IPV6,
                ace: vec![Ace {
                    name: "some_ace_name".to_string(),
                    action: AceAction::Accept,
                    matches: AceMatches {
                        protocol: Some(AceProtocol::Tcp),
                        direction_initiated: None,
                        address_mask: None,
                        dnsname: Some(String::from("www.example.test")),
                        source_port: None,
                        destination_port: None,
                    },
                }],
            }],
            acl_override: vec![Acl {
                name: "some_acl_name".to_string(),
                packet_direction: AclDirection::ToDevice,
                acl_type: AclType::IPV4,
                ace: vec![Ace {
                    name: "overriden_ace".to_string(),
                    action: AceAction::Deny,
                    matches: AceMatches {
                        protocol: Some(AceProtocol::Udp),
                        direction_initiated: None,
                        address_mask: None,
                        dnsname: Some(String::from("www.example.test")),
                        source_port: None,
                        destination_port: None,
                    },
                }],
            }],
        };

        let device = DeviceWithRefs {
            inner: Device {
                id: 0,
                name: None,
                mac_addr: Some("aa:bb:cc:dd:ee:ff".parse::<MacAddr>().unwrap().into()),
                duid: None,
                ipv4_addr: "127.0.0.1".parse().ok(),
                ipv6_addr: None,
                hostname: "".to_string(),
                vendor_class: "".to_string(),
                mud_url: Some("http://example.com/mud_url.json".to_string()),
                last_interaction: Utc::now().naive_utc(),
                collect_info: false,
                clipart: None,
                room_id: None,
            },
            mud_data: Some(mud_data),
            room: None,
        };

        let x = convert_device_to_fw_rules(&device);

        let resulting_device = FirewallDevice {
            id: device.id,
            ipv4_addr: device.ipv4_addr,
            ipv6_addr: device.ipv6_addr,
            rules: vec![
                FirewallRule::new(
                    RuleName::new(String::from("rule_0")),
                    RuleTarget::new(Some(RuleTargetHost::Hostname(String::from("www.example.test"))), None),
                    RuleTarget::new(Some(RuleTargetHost::FirewallDevice), None),
                    Protocol::Udp,
                    Verdict::Reject,
                ),
                FirewallRule::new(
                    RuleName::new(String::from("rule_default_1")),
                    RuleTarget::new(Some(RuleTargetHost::FirewallDevice), None),
                    RuleTarget::new(None, None),
                    Protocol::All,
                    Verdict::Reject,
                ),
                FirewallRule::new(
                    RuleName::new(String::from("rule_default_2")),
                    RuleTarget::new(None, None),
                    RuleTarget::new(Some(RuleTargetHost::FirewallDevice), None),
                    Protocol::All,
                    Verdict::Reject,
                ),
            ],
            collect_data: false,
        };

        assert!(x.eq(&resulting_device));

        Ok(())
    }

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
            expiration: Utc::now(),
            acllist: vec![Acl {
                name: "some_acl_name".to_string(),
                packet_direction: AclDirection::ToDevice,
                acl_type: AclType::IPV6,
                ace: vec![Ace {
                    name: "some_ace_name".to_string(),
                    action: AceAction::Accept,
                    matches: AceMatches {
                        protocol: Some(AceProtocol::Tcp),
                        direction_initiated: None,
                        address_mask: None,
                        dnsname: Some(String::from("www.example.test")),
                        source_port: None,
                        destination_port: None,
                    },
                }],
            }],
            acl_override: Vec::default(),
        };

        let device = DeviceWithRefs {
            inner: Device {
                id: 0,
                name: None,
                mac_addr: Some("aa:bb:cc:dd:ee:ff".parse::<MacAddr>().unwrap().into()),
                duid: None,
                ipv4_addr: "127.0.0.1".parse().ok(),
                ipv6_addr: None,
                hostname: "".to_string(),
                vendor_class: "".to_string(),
                mud_url: Some("http://example.com/mud_url.json".to_string()),
                collect_info: true,
                last_interaction: Utc::now().naive_utc(),
                clipart: None,
                room_id: None,
            },
            mud_data: Some(mud_data),
            room: None,
        };

        let x = convert_device_to_fw_rules(&device);

        let resulting_device = FirewallDevice {
            id: device.id,
            ipv4_addr: device.ipv4_addr,
            ipv6_addr: device.ipv6_addr,
            rules: vec![
                FirewallRule::new(
                    RuleName::new(String::from("rule_0")),
                    RuleTarget::new(Some(RuleTargetHost::Hostname(String::from("www.example.test"))), None),
                    RuleTarget::new(Some(RuleTargetHost::FirewallDevice), None),
                    Protocol::Tcp,
                    Verdict::Accept,
                ),
                FirewallRule::new(
                    RuleName::new(String::from("rule_default_1")),
                    RuleTarget::new(Some(RuleTargetHost::FirewallDevice), None),
                    RuleTarget::new(None, None),
                    Protocol::All,
                    Verdict::Reject,
                ),
                FirewallRule::new(
                    RuleName::new(String::from("rule_default_2")),
                    RuleTarget::new(None, None),
                    RuleTarget::new(Some(RuleTargetHost::FirewallDevice), None),
                    Protocol::All,
                    Verdict::Reject,
                ),
            ],
            collect_data: true,
        };

        assert!(x.eq(&resulting_device));

        Ok(())
    }
}
