extern crate env_logger;
#[macro_use]
extern crate quick_error;

mod config;
mod logic;
mod p2p;
use crate::p2p::join;
use async_channel;
use async_signals::Signals;
use futures_lite::stream::StreamExt;
use libp2p::identity::Keypair;

const CHANNEL_SIZE: usize = 256;

fn main() {
    let room_id = "@test";

    let (tap_send, _tap_receive) = async_channel::bounded(CHANNEL_SIZE);
    let (control_send, control_receive) = async_channel::bounded(CHANNEL_SIZE);

    let config = config::load().unwrap();
    let keypair = config.get_keypair().unwrap();

    smol::spawn(async move {
        let mut signals = Signals::new(vec![libc::SIGINT]).unwrap();
        signals.next().await;

        control_send.close();
    })
    .detach();

    smol::block_on(async {
        // control_send.send(ControlMessage::Stop).await.unwrap();

        let keypair = Keypair::Ed25519(keypair);

        join(keypair, room_id, control_receive, tap_send)
            .await
            .unwrap();
    })
}
