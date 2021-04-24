use base64;
use libp2p::identity::ed25519::Keypair;
use libp2p::identity::error::DecodingError;
use serde::{Deserialize, Serialize};
use serde_yaml;
use shellexpand;
use std::borrow::Borrow;
use std::env::VarError;
use std::fs::File;
use std::io::prelude::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    private_key: String,
    pub id: String,
}

type ExpandError = shellexpand::LookupError<VarError>;

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

fn expand(input: &str) -> Result<String, ExpandError> {
    let result = shellexpand::env(input)?;
    let result: &str = result.borrow();
    Ok(String::from(result))
}

pub fn load() -> Result<Config, LoadError> {
    let file_path = expand("$HOME/.config/synco")?;

    let file = match File::open(file_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return create(),
        Err(e) => return Err(e.into()),
        Ok(f) => f,
    };

    let config = serde_yaml::from_reader(file)?;
    Ok(config)
}

pub fn create() -> Result<Config, LoadError> {
    let file_path = expand("$HOME/.config/synco")?;
    let key = Keypair::generate().encode();

    println!("generated key length: {}", key.len());

    // create the resulting value
    let cfg = Config {
        private_key: base64::encode_config(key, base64::STANDARD_NO_PAD),
        id: expand("$USER")?,
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
