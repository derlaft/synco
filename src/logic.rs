use crate::channels;
use crate::mpv;
use crate::p2p;
use crate::proto;
use crate::statemachine;
use async_channel::{Receiver, SendError};
use futures_micro;
use log::{debug, info};
use smol::stream::StreamExt;
use smol::Timer;
use std::sync::Arc;
use std::time::Duration;

quick_error! {
    #[derive(Debug)]
    pub enum Error {
        LogicError(err: LogicError) {
            from()
        }
        SendNetworkMessageError(err: SendError<proto::Action>) {
            from()
        }
        SendStateMachineError(err: SendError<statemachine::Event>) {
            from()
        }
        SendMpvMessageError(err: SendError<mpv::Request>) {
            from()
        }
        JSONError(err: serde_json::Error) {
            from()
        }
        StateMachineError(err: statemachine::Error) {
            from()
        }
    }
}

#[derive(Debug)]
pub struct LogicError {
    reason: String,
}

struct Controller {
    channels: channels::LogicSenderChannels,
}

const UPKEEP_DUR: Duration = Duration::from_millis(500);

impl Controller {
    async fn consume_mpv_events(
        self: Arc<&Self>,
        mut from_mpv_receive: Receiver<mpv::Event>,
    ) -> Result<(), Error> {
        // first step: initialize mpv
        let init_seq = vec![
            mpv::Request::osd_overlay("synco v3: inititializing..."),
            mpv::Request::set_pause(true),
            mpv::Request::keybind("F1", "script_message ready_pressed"),
            mpv::Request::observe_property(mpv::Property::TimePos),
            mpv::Request::observe_property(mpv::Property::Speed),
            mpv::Request::observe_property(mpv::Property::Seeking),
        ];

        // send init commands
        for init_cmd in init_seq {
            self.channels.to_mpv_send.send(init_cmd).await?;
        }

        while let Some(event) = from_mpv_receive.next().await {
            debug!("consume_mpv_events next(): {:?}", event);
            self.channels
                .to_logic_send
                .send(statemachine::Event::Mpv(event))
                .await?;
        }

        Ok(())
    }

    async fn upkeep_timer(self: Arc<&Self>) -> Result<(), Error> {
        loop {
            self.channels
                .to_logic_send
                .send(statemachine::Event::KeepAlive)
                .await?;

            Timer::after(UPKEEP_DUR).await;
        }
    }

    async fn feed_logic(
        self: Arc<&Self>,
        mut r: Receiver<statemachine::Event>,
    ) -> Result<(), Error> {
        let mut state_machine = statemachine::StateMachine::new(
            self.channels.to_mpv_send.clone(),
            self.channels.to_network_send.clone(),
        );

        while let Some(event) = r.next().await {
            debug!("feed_logic next(): {:?}", event);
            state_machine.process_event(event).await?;
        }

        Ok(())
    }

    async fn consume_network_events(
        self: Arc<&Self>,
        mut from_network_receive: Receiver<(p2p::Peer, p2p::Message)>,
    ) -> Result<(), Error> {
        while let Some((from, msg)) = from_network_receive.next().await {
            // parse msg
            info!(
                "consume_network_events: message from {}@{}: {:?}",
                from, msg.user_id, msg.action,
            );

            self.channels
                .to_logic_send
                .send(statemachine::Event::Network(msg))
                .await?;
        }

        Ok(())
    }
}

// remote events handlers
impl Controller {}

pub async fn logic_controller(channels: channels::LogicChannels) -> Result<(), Error> {
    // pure madness controller

    let c = Controller {
        channels: channels.senders(),
    };
    let c = Arc::new(&c);

    let from_mpv_receive = channels.from_mpv_receive;
    let from_network_receive = channels.from_network_receive;
    let from_logic_receive = channels.from_logic_receive;

    futures_micro::or!(
        c.clone().consume_mpv_events(from_mpv_receive),
        c.clone().consume_network_events(from_network_receive),
        c.clone().upkeep_timer(),
        c.clone().feed_logic(from_logic_receive),
    )
    .await
}
