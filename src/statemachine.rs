use crate::mpv;
use crate::proto;
use async_channel::{SendError, Sender};
use log::error;
use log::warn;
use std::collections::HashMap;

const SEEK_IGNORE_THRES: f64 = 0.2;

const MAX_DESYNC: f64 = 2.0;

pub struct StateMachine {
    to_mpv_send: Sender<mpv::Request>,
    to_network_send: Sender<proto::Action>,
    global_state: HashMap<String, State>,
    local_state: State,
    network_ready: bool,
}

#[derive(Default)]
struct State {
    position: f64,
    speed: f64,
    ready: bool,
    paused: bool,
    seeking: bool,
    please_skip_sync_check: bool,
}

quick_error! {
    #[derive(Debug)]
    pub enum Error {
        SendActionError(err: SendError<proto::Action>) {
            from()
        }
        SendMpvRequest(err: SendError<mpv::Request>) {
            from()
        }
    }
}

#[derive(Debug)]
pub enum Event {
    Mpv(mpv::Event),
    Network(proto::Message),
    KeepAlive,
}

impl StateMachine {
    pub fn new(
        to_mpv_send: Sender<mpv::Request>,
        to_network_send: Sender<proto::Action>,
    ) -> StateMachine {
        let sm = StateMachine {
            to_mpv_send,
            to_network_send,
            global_state: HashMap::new(),
            local_state: State::default(),
            network_ready: false,
        };

        sm
    }

    async fn start_to_be_ready(&mut self) -> Result<(), Error> {
        self.local_state.ready = true;
        self.to_mpv_send
            .send(mpv::Request::display_message("ready"))
            .await?;
        self.to_network_send.send(proto::Action::Ready).await?;

        Ok(())
    }

    async fn stop_being_ready(&mut self, reason: &str) -> Result<(), Error> {
        self.local_state.ready = false;
        self.to_network_send.send(proto::Action::Unready).await?;

        self.to_mpv_send
            .send(mpv::Request::display_message(
                format!("not ready: {}", reason).as_str(),
            ))
            .await?;

        Ok(())
    }

    async fn start_playback(&mut self) -> Result<(), Error> {
        self.local_state.paused = false;

        self.to_mpv_send
            .send(mpv::Request::set_pause(false))
            .await?;
        self.to_mpv_send
            .send(mpv::Request::display_message("go"))
            .await?;

        Ok(())
    }

    async fn stop_playback(&mut self, reason: &str) -> Result<(), Error> {
        if !self.local_state.paused {
            self.local_state.paused = true;
            self.to_mpv_send.send(mpv::Request::set_pause(true)).await?;
        }

        if self.local_state.ready {
            self.stop_being_ready(reason).await?;
        }

        self.to_mpv_send
            .send(mpv::Request::display_message(
                format!("stopped playback: {}", reason).as_str(),
            ))
            .await?;

        Ok(())
    }

    async fn suppress_unpause(&mut self, reason: &str) -> Result<(), Error> {
        self.to_mpv_send.send(mpv::Request::set_pause(true)).await?;
        self.stop_being_ready(reason).await?;

        Ok(())
    }

    async fn on_other_join(&mut self, name: &str) -> Result<(), Error> {
        self.to_mpv_send
            .send(mpv::Request::display_message(
                format!("Oh hi {}", name).as_str(),
            ))
            .await?;

        Ok(())
    }

    pub async fn process_event(&mut self, event: Event) -> Result<(), Error> {
        match event {
            Event::KeepAlive => {
                self.to_network_send
                    .send(proto::Action::Position {
                        pos: self.local_state.position,
                    })
                    .await?;

                // check desync
                if !self.local_state.paused
                    && !self.local_state.seeking
                    && !self.local_state.please_skip_sync_check
                {
                    let desync = self
                        .global_state
                        .values()
                        .map(|v| (v.position - self.local_state.position).abs())
                        .max_by(|a, b| a.partial_cmp(b).unwrap())
                        .unwrap_or_default();

                    if desync > MAX_DESYNC {
                        self.stop_playback(format!("desync of {} detected", desync).as_str())
                            .await?;
                    }
                }

                self.local_state.please_skip_sync_check = false
            }
            Event::Mpv(event) => match event {
                // handle command responses
                mpv::Event::SuccessResponse { .. } => (), // ignore
                mpv::Event::ErrorResponse { error, .. } => {
                    // TODO: maybe we want to abort here
                    error!("statemachine: command response error: {}", error);
                }
                mpv::Event::Seek => {
                    // self.local_state.seeking = true,
                }
                mpv::Event::FloatPropertyChange { property, value } => match property {
                    mpv::FloatProperty::TimePos => {
                        self.local_state.position = value;
                        if self.local_state.seeking {
                            self.local_state.seeking = false;

                            self.to_network_send
                                .send(proto::Action::Seek {
                                    pos: self.local_state.position,
                                })
                                .await?;
                            self.local_state.please_skip_sync_check = false
                        }
                    }
                    mpv::FloatProperty::Speed => self.local_state.speed = value,
                },
                mpv::Event::ClientMessage { id } if id == "ready_pressed" => {
                    if self.local_state.ready {
                        self.stop_being_ready("local unready").await?;
                    } else {
                        self.start_to_be_ready().await?;

                        // also start playback if everyone is ready
                        if self.network_ready {
                            self.start_playback().await?;
                        }
                    };
                }
                mpv::Event::ClientMessage { .. } => {} // ignore all other client msgs
                mpv::Event::Event { event } if event == "pause" => {
                    // mpv is already paused
                    self.local_state.paused = true;

                    if self.local_state.ready {
                        self.stop_being_ready("local pause").await?;
                    }
                }
                mpv::Event::Event { event } if event == "unpause" => {
                    // mpv is already unpaused
                    self.local_state.paused = false;

                    if !self.local_state.ready {
                        self.suppress_unpause("not ready").await?;
                    }

                    if !self.network_ready {
                        self.suppress_unpause("network is not ready").await?;
                    }
                }
                mpv::Event::Event { event } if event == "playback-restart" => {
                    self.local_state.seeking = true;
                }
                mpv::Event::Event { event } => {
                    warn!("statemachine: unknown event_type: {}", event);
                }
            },
            Event::Network(event) => {
                let from = event.user_id.clone(); // TODO

                // TODO: this is ugly
                // (well, as ugly as the rest of ugly rust)
                if !self.global_state.contains_key(&from) {
                    let new_state = State::default();
                    self.global_state.insert(from.clone(), new_state);
                    self.on_other_join(from.clone().as_str()).await?;
                }
                let node_state = &mut self.global_state.get_mut(&from.clone()).unwrap();

                match event.action {
                    proto::Action::Hello => {
                        node_state.ready = false;
                        self.network_ready = false;

                        self.on_other_join(from.clone().as_str()).await?;
                        self.stop_playback(format!("{} joined", from).as_str())
                            .await?;
                    }

                    proto::Action::Ready => {
                        node_state.ready = true;
                        self.network_ready = self.global_state.len() > 0
                            && self.global_state.values().all(|s| s.ready);

                        if self.local_state.ready && self.network_ready {
                            self.start_playback().await?;
                        }
                    }

                    proto::Action::Unready => {
                        self.network_ready = false;
                        node_state.ready = false;

                        if !self.local_state.paused {
                            self.stop_playback(format!("{} is not ready", event.user_id).as_str())
                                .await?;
                        }
                    }

                    proto::Action::Seek { pos } => {
                        node_state.position = pos;

                        if (node_state.position - self.local_state.position).abs()
                            > SEEK_IGNORE_THRES
                        {
                            self.to_mpv_send.send(mpv::Request::seek(pos)).await?;
                            self.local_state.please_skip_sync_check = true;
                        }
                    }

                    proto::Action::Position { pos } => {
                        node_state.position = pos;
                    }

                    proto::Action::Speed { speed } => {
                        node_state.speed = speed;
                    }
                }
            }
        };

        Ok(())
    }
}
