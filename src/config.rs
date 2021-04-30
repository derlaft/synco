use crate::util::{expand, ExpandError};
use base64;
use libp2p::identity::ed25519::Keypair;
use libp2p::identity::error::DecodingError;
use log::debug;
use serde::{Deserialize, Serialize};
use serde_yaml;
use std::fs::File;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    private_key: String,
    pub id: String,
    pub listen_on: Vec<String>,
}

quick_error! {
    #[derive(Debug)]
    pub enum LoadError {
        IOError(err: std::io::Error) {
            from()
        }
        ExpandError(err: ExpandError) {
            from()
        }
        YamlError(err: serde_yaml::Error) {
            from()
        }
    }
}

quick_error! {
    #[derive(Debug)]
    pub enum DecodeKeypairError {
        Base64Error(err: base64::DecodeError) {
            from()
        }
        DecodeError(err: DecodingError) {
            from()
        }
    }
}

fn get_config_path() -> Result<String, LoadError> {
    let mut file_path = std::env::var("SYNCO_CONFIG").unwrap_or_default();
    if file_path.is_empty() {
        file_path = expand("$HOME/.config/synco")?;
    };
    Ok(file_path)
}

pub fn load() -> Result<Config, LoadError> {
    let file_path = get_config_path()?;

    let file = match File::open(file_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return create(),
        Err(e) => return Err(e.into()),
        Ok(f) => f,
    };

    let config = serde_yaml::from_reader(file)?;
    Ok(config)
}

pub fn create() -> Result<Config, LoadError> {
    let file_path = get_config_path()?;
    let key = Keypair::generate().encode();

    debug!("generated key length: {}", key.len());

    // create the resulting value
    let cfg = Config {
        private_key: base64::encode_config(key, base64::STANDARD_NO_PAD),
        id: expand("$USER")?,
        listen_on: vec![
            "/ip4/0.0.0.0/tcp/0".to_string(),
            "/ip6/::/tcp/0".to_string(),
        ],
    };

    // save to disk
    let file = File::create(file_path)?;
    serde_yaml::to_writer(file, &cfg)?;

    // nya
    Ok(cfg)
}

impl Config {
    pub fn get_keypair(&self) -> Result<Keypair, DecodeKeypairError> {
        let mut key_data = base64::decode(&self.private_key)?;
        let key = Keypair::decode(key_data.as_mut_slice())?;
        Ok(key)
    }
}
