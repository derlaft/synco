extern crate env_logger;
#[macro_use]
extern crate quick_error;

mod channels;
mod config;
mod logic;
mod mpv;
mod p2p;
mod proto;
mod statemachine;
mod util;

use crate::p2p::join;
use log::error;

fn main() {
    env_logger::init();

    let config = config::load().unwrap();

    smol::block_on(async {
        join(config, None)
            .await
            .unwrap_or_else(move |e| error!("p2p worker error, shutting down: {}", e));
    });
}
