#![allow(clippy::field_reassign_with_default)]

use argon2::{self, Config};
use paperclip::actix::Apiv2Schema;
use rand::{rngs::OsRng, Rng};
use snafu::ensure;

use crate::{error, error::Result};

pub const SALT_LENGTH: usize = 32;

#[derive(Debug, Clone)]
pub struct UserDbo {
    pub id: i64,
    pub username: String,
    pub password: String,
    pub salt: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Apiv2Schema)]
pub struct User {
    pub id: i64,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    #[serde(skip_serializing)]
    pub salt: Vec<u8>,
    pub roles: Vec<String>,
    pub roles_ids: Vec<i64>,
    pub permissions: Vec<String>,
}

#[derive(Debug)]
pub struct RoleDbo {
    pub id: i64,
    pub name: String,
    pub permissions: String,
}

// Local methods
impl User {
    pub fn new(username: String, password: &str) -> Result<Self> {
        let mut salt = vec![0u8; SALT_LENGTH];
        OsRng::default().fill(salt.as_mut_slice());

        Ok(Self {
            id: 0,
            username,
            password: User::hash_password(password, &salt)?,
            salt,
            roles: Vec::new(),
            roles_ids: Vec::new(),
            permissions: Vec::new(),
        })
    }

    pub fn verify_password(&self, password: &str) -> Result<()> {
        let result = argon2::verify_encoded(self.password.as_ref(), password.as_ref())?;
        ensure!(result, error::PasswordVerifyError {});
        Ok(())
    }

    pub fn hash_password(password: &str, salt: &[u8]) -> Result<String> {
        let argon_config = Config::default();

        Ok(argon2::hash_encoded(password.as_bytes(), salt, &argon_config)?)
    }
}
