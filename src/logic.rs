use crate::mpv;
use crate::p2p;
use async_channel::{Receiver, SendError, Sender};
use smol::stream::StreamExt;
use std::sync::Arc;

quick_error! {
    #[derive(Debug)]
    pub enum Error {
        LogicError(err: LogicError) {
            from()
        }
        SendNetworkMessageError(err: SendError<p2p::Message>) {
            from()
        }
        SendMpvMessageError(err: SendError<mpv::Request>) {
            from()
        }
    }
}

#[derive(Debug)]
pub struct LogicError {
    reason: String,
}

struct Controller {
    to_mpv_send: Sender<mpv::Request>,
    to_network_send: Sender<p2p::Message>,
}

impl Controller {
    async fn consume_mpv_events(
        self: Arc<&Self>,
        from_mpv_receive: &mut Receiver<mpv::Event>,
    ) -> Result<(), Error> {
        // first step: initialize mpv
        let init_seq = vec![
            mpv::Request::set_pause(true),
            mpv::Request::keybind("F1", "script_message ready_pressed"),
            mpv::Request::observe_property(mpv::Property::TimePos),
            mpv::Request::observe_property(mpv::Property::Speed),
            // mpv::Request::observe_property(mpv::Property::Seeking),
        ];

        // send init commands
        for init_cmd in init_seq {
            self.to_mpv_send.send(init_cmd).await?;
        }

        while let Some(event) = from_mpv_receive.next().await {
            eprintln!("Got parsed event: {:?}", event);
        }
        Ok(())
    }

    async fn consume_network_events(
        self: Arc<&Self>,
        from_network_receive: &mut Receiver<(p2p::Peer, p2p::Message)>,
    ) -> Result<(), Error> {
        while let Some(msg) = from_network_receive.next().await {
            // huge TODO

            // add msg to logic
            let _ = msg;
            let example_network_msg = "{\"type\": \"consume_mpv_event_fired\"}"
                .as_bytes()
                .to_vec();

            // send logic result to network
            self.to_network_send.send(example_network_msg).await?;

            // send logic result to mpv
            self.to_mpv_send.send(mpv::Request::set_pause(true)).await?;
        }

        Ok(())
    }
}

pub async fn logic_controller(
    from_mpv_receive: &mut Receiver<mpv::Event>,
    to_mpv_send: Sender<mpv::Request>,
    from_network_receive: &mut Receiver<(p2p::Peer, p2p::Message)>,
    to_network_send: Sender<p2p::Message>,
) -> Result<(), Error> {
    // pure madness controller

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
