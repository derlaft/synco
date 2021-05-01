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

use crate::logic::logic_controller;
use crate::p2p::join;
use futures_micro;
use log::error;

fn main() {
    env_logger::init();

    let config = config::load().unwrap();

    let mut channels = channels::SyncoChannels::new_client();

    let mpv_channels = &mut channels.mpv;
    let p2p_channels = channels.p2p;
    let logic_channels = &mut channels.logic;

    smol::block_on(futures_micro::zip!(
        // mpv worker
        async {
            mpv::start(mpv_channels)
                .await
                .unwrap_or_else(move |e| error!("mpv worker error, shutting down: {}", e));
        },
        // p2p worker
        async {
            join(config, Some(p2p_channels))
                .await
                .unwrap_or_else(move |e| error!("p2p worker error, shutting down: {}", e));
        },
        // logic controller
        async {
            logic_controller(logic_channels)
                .await
                .unwrap_or_else(move |e| error!("logic worker error, shutting down: {}", e));
        },
    ));
}
