/*
 * No description provided (generated by Openapi Generator https://github.com/openapitools/openapi-generator)
 *
 * The version of the OpenAPI document: 0.0.0
 * 
 * Generated by: https://openapi-generator.tech
 */




#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thing {
    #[serde(rename = "serial")]
    pub serial: String,
    #[serde(rename = "mac_addr")]
    pub mac_addr: String,
    #[serde(rename = "ipv4_addr")]
    pub ipv4_addr: String,
    #[serde(rename = "ipv6_addr")]
    pub ipv6_addr: String,
    #[serde(rename = "hostname")]
    pub hostname: String,
}

impl Thing {
    pub fn new(serial: String, mac_addr: String, ipv4_addr: String, ipv6_addr: String, hostname: String) -> Thing {
        Thing {
            serial,
            mac_addr,
            ipv4_addr,
            ipv6_addr,
            hostname,
        }
    }
}


