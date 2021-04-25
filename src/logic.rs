use crate::mpv;
use crate::p2p;
use async_channel::{Receiver, Sender};
use smol::stream::StreamExt;
use std::sync::Arc;

quick_error! {
    #[derive(Debug)]
    pub enum Error {
        LogicError(err: LogicError) {
            from()
        }
    }
}

#[derive(Debug)]
pub struct LogicError {
    reason: String,
}

struct Controller {
    to_mpv_send: Sender<mpv::Message>,
    to_network_send: Sender<p2p::Message>,
}

impl Controller {
    async fn consume_mpv_events(
        self: Arc<&Self>,
        from_mpv_receive: &mut Receiver<mpv::Message>,
    ) -> Result<(), Error> {
        while let Some(msg) = from_mpv_receive.next().await {}
        Ok(())
    }

    async fn consume_network_events(
        self: Arc<&Self>,
        from_network_receive: &mut Receiver<(p2p::Peer, p2p::Message)>,
    ) -> Result<(), Error> {
        Ok(())
    }
}

pub async fn logic_controller(
    from_mpv_receive: &mut Receiver<mpv::Message>,
    to_mpv_send: Sender<mpv::Message>,
    from_network_receive: &mut Receiver<(p2p::Peer, p2p::Message)>,
    to_network_send: Sender<p2p::Message>,
) -> Result<(), Error> {
    // pure madness here

    let c = Controller {
        to_mpv_send,
        to_network_send,
    };
    let c = Arc::new(&c);

    smol::future::or(
        c.clone().consume_mpv_events(from_mpv_receive),
        c.clone().consume_network_events(from_network_receive),
    )
    .await
}
