#![allow(clippy::field_reassign_with_default)]

use paperclip::actix::Apiv2Schema;

/// The config has a very simple structure. A key (which is also a the primary key) and a value (which is a string)
/// You can use it to store anything you need as Key-Values in the Database
#[derive(Debug, Serialize, Deserialize, Clone, Apiv2Schema, Eq, PartialEq)]
pub struct Config {
    pub key: String,
    pub value: String,
}
