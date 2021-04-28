extern crate env_logger;
#[macro_use]
extern crate quick_error;

mod config;
mod logic;
mod mpv;
mod p2p;
mod proto;
mod statemachine;
mod util;

use crate::logic::logic_controller;
use crate::p2p::join;
use async_channel;
use async_signals::Signals;
use futures_lite::stream::StreamExt;
use libp2p::identity::Keypair;

const CHANNEL_SIZE: usize = 256;

fn main() {
    let room_id = "@test";

    let (from_network_send, mut from_network_receive) = async_channel::bounded(CHANNEL_SIZE);
    let (to_network_send, mut to_network_receive) = async_channel::bounded(CHANNEL_SIZE);

    let (to_mpv_send, mut to_mpv_receive) = async_channel::bounded(CHANNEL_SIZE);
    let (from_mpv_send, mut from_mpv_receive) = async_channel::bounded(CHANNEL_SIZE);

    let config = config::load().unwrap();
    let user_id = config.id.clone();
    let keypair = {
        let keypair = config.get_keypair().unwrap();
        Keypair::Ed25519(keypair)
    };

    smol::block_on(smol::future::zip(
        // mpv worker
        async {
            mpv::start(&mut to_mpv_receive, from_mpv_send)
                .await
                .unwrap_or_else(move |e| println!("mpv worker error, shutting down: {}", e));
        },
        smol::future::zip(
            // p2p worker
            async {
                join(
                    keypair,
                    user_id.clone().as_str(),
                    room_id,
                    to_network_receive,
                    from_network_send,
                )
                .await
                .unwrap_or_else(move |e| println!("p2p worker error, shutting down: {}", e));
            },
            // logic controller
            async {
                logic_controller(
                    config.id,
                    &mut from_mpv_receive,
                    to_mpv_send,
                    &mut from_network_receive,
                    to_network_send,
                )
                .await
                .unwrap_or_else(move |e| println!("logic worker error, shutting down: {}", e));
            },
        ),
    ));
}
