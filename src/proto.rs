use serde::{self, Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t")]
pub enum Action {
    #[serde(rename = "h")]
    Hello,

    // msgs
    #[serde(rename = "r")]
    Ready,

    #[serde(rename = "!r")]
    Unready,

    #[serde(rename = ">>")]
    Seek {
        #[serde(rename = "p")]
        #[serde(default)]
        pos: f64,
    },

    #[serde(rename = "??")]
    Position {
        #[serde(rename = "p")]
        #[serde(default)]
        pos: f64,
    },

    #[serde(rename = ">>>")]
    Speed {
        #[serde(rename = "s")]
        #[serde(default)]
        speed: f64,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Message {
    #[serde(rename = "u")]
    pub user_id: String,

    #[serde(flatten)]
    pub action: Action,
}
