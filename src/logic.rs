use crate::mpv;
use crate::p2p;
use crate::proto;
use crate::statemachine;
use async_channel::{Receiver, SendError, Sender};
use smol::lock::Mutex;
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
    to_mpv_send: Sender<mpv::Request>,
    to_network_send: Sender<p2p::Action>,
    to_logic_send: Sender<statemachine::Event>,
}

const UPKEEP_DUR: Duration = Duration::from_millis(1000);

impl Controller {
    async fn consume_mpv_events(
        self: Arc<&Self>,
        from_mpv_receive: &mut Receiver<mpv::Event>,
    ) -> Result<(), Error> {
        // first step: initialize mpv
        let init_seq = vec![
            mpv::Request::display_message("synco v3"),
            mpv::Request::set_pause(true),
            mpv::Request::keybind("F1", "script_message ready_pressed"),
            mpv::Request::observe_property(mpv::Property::TimePos),
            mpv::Request::observe_property(mpv::Property::Speed),
            mpv::Request::observe_property(mpv::Property::Seeking),
        ];

        // send init commands
        for init_cmd in init_seq {
            self.to_mpv_send.send(init_cmd).await?;
        }

        while let Some(event) = from_mpv_receive.next().await {
            eprintln!("Got parsed event: {:?}", event);
            // TODO
            self.to_logic_send
                .send(statemachine::Event::Mpv(event))
                .await?;
        }

        Ok(())
    }

    async fn upkeep_timer(self: Arc<&Self>) -> Result<(), Error> {
        loop {
            self.to_logic_send
                .send(statemachine::Event::KeepAlive)
                .await?;
            Timer::after(UPKEEP_DUR).await;
        }
    }

    async fn feed_logic(
        self: Arc<&Self>,
        r: &mut Receiver<statemachine::Event>,
    ) -> Result<(), Error> {
        let mut state_machine =
            statemachine::StateMachine::new(self.to_mpv_send.clone(), self.to_network_send.clone());

        while let Some(event) = r.next().await {
            state_machine.process_event(event).await?;
        }

        Ok(())
    }

    async fn consume_network_events(
        self: Arc<&Self>,
        from_network_receive: &mut Receiver<(p2p::Peer, p2p::Message)>,
    ) -> Result<(), Error> {
        while let Some((from, msg)) = from_network_receive.next().await {
            // parse msg
            eprintln!(
                "Received a message from {}@{}: {:?}",
                from, msg.user_id, msg.action,
            );

            self.to_logic_send
                .send(statemachine::Event::Network(msg))
                .await?;
        }

        Ok(())
    }
}

// remote events handlers
impl Controller {}

pub async fn logic_controller(
    user_id: String,
    from_mpv_receive: &mut Receiver<mpv::Event>,
    to_mpv_send: Sender<mpv::Request>,
    from_network_receive: &mut Receiver<(p2p::Peer, p2p::Message)>,
    to_network_send: Sender<p2p::Action>,
) -> Result<(), Error> {
    // pure madness controller
    let (to_logic_send, mut from_logic_receive) = async_channel::bounded(256);

    let c = Controller {
        to_mpv_send,
        to_network_send,
        to_logic_send,
    };
    let c = Arc::new(&c);

    smol::future::or(
        smol::future::or(
            smol::future::or(
                c.clone().consume_mpv_events(from_mpv_receive),
                c.clone().consume_network_events(from_network_receive),
            ),
            c.clone().upkeep_timer(),
        ),
        c.clone().feed_logic(&mut from_logic_receive),
    )
    .await
}
