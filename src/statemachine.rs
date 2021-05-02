use crate::mpv;
use crate::proto;
use async_channel::{SendError, Sender};
use log::debug;
use log::error;
use log::warn;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, SystemTimeError};

const SEEK_IGNORE_THRES: f64 = 0.5;

const MAX_DESYNC: f64 = 1.5;

const SKIP_DESYNC_AFTER_SEEK_S: f64 = 2.0;

const LOG_DISPLAY_SEC: Duration = Duration::from_secs(1);

type NetworkTarget = (SystemTime, f64);

pub struct StateMachine {
    to_mpv_send: Sender<mpv::Request>,
    to_network_send: Sender<proto::Action>,
    global_state: HashMap<String, State>,
    local_state: State,
    network_ready: bool,
    log: Vec<LogEntry>,
    network_seek_target: Option<NetworkTarget>,
    last_seek: Instant,
    send_remote_seek_on_next_pos: bool,
    last_seekrelated_event: Instant,
}

#[derive(Default)]
struct State {
    position: f64,
    speed: f64,
    ready: bool,
    paused: bool,
    seeking: bool, // only used for local state
}

#[derive(Clone)]
struct LogEntry {
    when: Instant,
    what: String,
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
        SystemTimeError(err: SystemTimeError) {
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
        let mut sm = StateMachine {
            to_mpv_send,
            to_network_send,
            global_state: HashMap::new(),
            local_state: State::default(),
            network_ready: false,
            log: vec![],
            network_seek_target: None,
            last_seek: Instant::now(),
            send_remote_seek_on_next_pos: false,
            last_seekrelated_event: Instant::now(),
        };

        // should be paused at startup
        sm.local_state.paused = true;

        sm
    }

    async fn start_to_be_ready(&mut self) -> Result<(), Error> {
        self.local_state.ready = true;

        self.log.push(LogEntry {
            when: Instant::now(),
            what: "you are ready".to_string(),
        });

        self.display_osd().await?;

        self.to_network_send.send(proto::Action::Ready).await?;

        Ok(())
    }

    async fn stop_being_ready(&mut self, reason: &str) -> Result<(), Error> {
        self.local_state.ready = false;
        self.to_network_send.send(proto::Action::Unready).await?;

        self.log.push(LogEntry {
            when: Instant::now(),
            what: format!("you not ready: {}", reason),
        });

        self.display_osd().await?;

        Ok(())
    }

    async fn start_playback(&mut self) -> Result<(), Error> {
        if self.local_state.paused {
            self.local_state.paused = false;
            self.network_seek_target = None;
        }

        self.to_mpv_send
            .send(mpv::Request::set_pause(false))
            .await?;

        self.log.push(LogEntry {
            when: Instant::now(),
            what: format!("everyone is ready, go"),
        });

        self.display_osd().await?;

        Ok(())
    }

    async fn stop_playback(&mut self, reason: &str) -> Result<(), Error> {
        if !self.local_state.paused {
            self.local_state.paused = true;
            self.network_seek_target = None;
            self.to_mpv_send.send(mpv::Request::set_pause(true)).await?;
        }

        if self.local_state.ready {
            self.stop_being_ready(reason).await?;
        }

        self.log.push(LogEntry {
            when: Instant::now(),
            what: format!("stopped playback: {}", reason),
        });

        self.display_osd().await?;

        Ok(())
    }

    fn calculated_pos(&mut self) -> Result<Option<f64>, Error> {
        Ok(if let Some(target) = self.network_seek_target {
            let diff = match self.local_state.paused {
                false => SystemTime::now().duration_since(target.0)?.as_secs_f64(),
                true => 0.0,
            };

            debug!(
                "duration since: {}, new calculated pos: {}",
                diff,
                target.1 + diff
            );

            Some(target.1 + diff)
        } else {
            None
        })
    }

    async fn seeking_target_check(&mut self) -> Result<(), Error> {
        debug!("WAT seeking_target_check");

        if let Some(calc_pos) = self.calculated_pos()? {
            let pos_diff = (calc_pos - self.local_state.position).abs();

            debug!("WAT seeking_target_check pos_diff={}", pos_diff);

            if pos_diff > SEEK_IGNORE_THRES {
                // if seek is outside, we have to assume we scrolled
                // notify everyone else
                self.send_remote_seek_on_next_pos = true;
            }
        } else {
            // no recorded network pos - for sure it's local seek
            debug!(
                "WAT sending seek to network, pos={}",
                self.local_state.position
            );
            self.send_remote_seek_on_next_pos = true;
        }

        Ok(())
    }

    async fn suppress_unpause(&mut self, reason: &str) -> Result<(), Error> {
        self.to_mpv_send.send(mpv::Request::set_pause(true)).await?;
        self.stop_being_ready(reason).await?;

        Ok(())
    }

    async fn on_other_join(&mut self, name: &str) -> Result<(), Error> {
        self.log.push(LogEntry {
            when: Instant::now(),
            what: format!("oh hi {}", name),
        });

        self.display_osd().await?;

        Ok(())
    }

    async fn display_osd(&mut self) -> Result<(), Error> {
        // remove old log messages
        self.log = self
            .log
            .clone()
            .into_iter()
            .filter(|x| x.when.elapsed() < LOG_DISPLAY_SEC)
            .collect();

        if self.log.len() == 0 && !self.local_state.paused {
            // clear overlay
            self.to_mpv_send.send(mpv::Request::osd_overlay("")).await?;
            return Ok(());
        }

        let local_state = format!(
            "ready: {}you{{\\r}} {}network{{\\r}}",
            match self.local_state.ready {
                false => "{\\c&HFF&}",
                true => "{\\c&HFF00&}",
            },
            match self.network_ready {
                false => "{\\c&HFF&}",
                true => "{\\c&HFF00&}",
            },
        );

        let remote_state = self
            .global_state
            .iter()
            .map(|x| {
                format!(
                    "{}{}{{\\r}}",
                    match x.1.ready {
                        false => "{\\c&HFF&}",
                        true => "{\\c&HFF00&}",
                    },
                    x.0,
                )
            })
            .collect::<Vec<String>>()
            .join("\\N");

        let log_msgs = self
            .log
            .clone()
            .into_iter()
            .rev()
            .map(|x| format!("{{\\fs40}}{}{{\\r}}", x.what))
            .collect::<Vec<String>>()
            .join("\\N");

        let total_state = format!("\\N\\N{}\\N{}\\N{}", local_state, remote_state, log_msgs);

        self.to_mpv_send
            .send(mpv::Request::osd_overlay(total_state.as_str()))
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

                let recent_network_seek = match self.network_seek_target {
                    None => false,
                    Some(v) => v.0.elapsed()?.as_secs_f64() < SKIP_DESYNC_AFTER_SEEK_S,
                };

                debug!("WAT: recent_network_seek: {}", recent_network_seek);

                // check desync
                if !self.local_state.paused
                    && self.last_seekrelated_event.elapsed().as_secs_f64()
                        > SKIP_DESYNC_AFTER_SEEK_S
                {
                    debug!("WAT: actually checking desync!",);

                    let desync = self
                        .global_state
                        .iter()
                        .map(|v| {
                            (
                                v.0.clone(), // from
                                (v.1.position - self.local_state.position).abs(),
                            )
                        })
                        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

                    if let Some(desync) = desync {
                        if desync.1 > MAX_DESYNC {
                            self.stop_playback(
                                format!(
                                    "{} is out of sync by {}, stopping",
                                    desync.0.clone(),
                                    desync.1
                                )
                                .as_str(),
                            )
                            .await?;
                        }
                    }
                }

                self.display_osd().await?;
            }
            Event::Mpv(event) => match event {
                // handle command responses
                mpv::Event::SuccessResponse { .. } => (), // ignore
                mpv::Event::ErrorResponse { error, .. } => {
                    // TODO: maybe we want to abort here
                    error!("statemachine: command response error: {}", error);
                }
                mpv::Event::Seek => {
                    self.last_seekrelated_event = Instant::now();
                }
                mpv::Event::BoolPropertyChange { property, value } => match property {
                    mpv::BoolProperty::Seeking => {
                        self.local_state.seeking = value;
                        self.last_seekrelated_event = Instant::now();
                    }
                },
                mpv::Event::FloatPropertyChange { property, value } => match property {
                    mpv::FloatProperty::TimePos => {
                        self.local_state.position = value;

                        if self.send_remote_seek_on_next_pos {
                            debug!("WAT seeking kostyl {}", self.local_state.position);
                            self.to_network_send
                                .send(proto::Action::Seek {
                                    pos: match self.local_state.position.is_sign_positive() {
                                        true => self.local_state.position,
                                        false => 0.0,
                                    },
                                })
                                .await?;
                            self.send_remote_seek_on_next_pos = false;
                        }
                    }
                    mpv::FloatProperty::Speed => {
                        self.local_state.speed = value;
                        self.to_network_send
                            .send(proto::Action::Speed { speed: value })
                            .await?;
                    }
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
                    self.network_seek_target = None;

                    if self.local_state.ready {
                        self.stop_being_ready("local pause").await?;
                    }
                }
                mpv::Event::Event { event } if event == "unpause" => {
                    // mpv is already unpaused
                    self.local_state.paused = false;
                    self.network_seek_target = None;

                    if !self.local_state.ready {
                        self.suppress_unpause("not ready").await?;
                    }

                    if !self.network_ready {
                        self.suppress_unpause("network is not ready").await?;
                    }
                }
                mpv::Event::Event { event } if event == "playback-restart" => {
                    self.seeking_target_check().await?;
                }
                mpv::Event::Event { event } => {
                    warn!("statemachine: unknown event_type: {}", event);
                }
            },
            Event::Network(event) => {
                let from = event.user_id.clone(); // TODO

                // TODO: this is ugly
                // (well, as ugly as the rest of ugly rust)
                // maybe this can be done on peer subscribe/unsubscribe handler
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

                        self.log.push(LogEntry {
                            when: Instant::now(),
                            what: format!("{} is ready", from),
                        });

                        self.display_osd().await?;
                    }

                    proto::Action::Unready => {
                        self.network_ready = false;
                        node_state.ready = false;

                        if !self.local_state.paused {
                            self.stop_playback(format!("{} is not ready", event.user_id).as_str())
                                .await?;
                        }

                        self.log.push(LogEntry {
                            when: Instant::now(),
                            what: format!("{} is not ready", from),
                        });

                        self.display_osd().await?;
                    }

                    proto::Action::Seek { pos } => {
                        self.last_seekrelated_event = Instant::now();
                        node_state.position = pos;
                        self.last_seek = Instant::now();

                        debug!("WAT received remote seek to {}", pos);

                        let restored_ts = SystemTime::UNIX_EPOCH + Duration::from_millis(event.ts);
                        self.network_seek_target = match self.network_seek_target {
                            Some(old_target) if old_target.0 > restored_ts => Some(old_target),
                            _ => Some((restored_ts, pos)),
                        };

                        if let Some(new_pos) = self.calculated_pos()? {
                            let diff = (new_pos - self.local_state.position).abs();
                            if diff > SEEK_IGNORE_THRES {
                                debug!("WAT diff is {}, seeking", diff);
                                self.to_mpv_send.send(mpv::Request::seek(new_pos)).await?;
                            } else {
                                debug!("WAT not reaching threshold");
                            }
                        } else {
                            debug!("WAT calculated pos is none");
                        }

                        debug!(
                            "WAT remote seek: new seek target is {:?}",
                            self.network_seek_target
                        );
                    }

                    proto::Action::Position { pos } => {
                        node_state.position = pos;
                    }

                    proto::Action::Speed { speed } => {
                        node_state.speed = speed;

                        if (self.local_state.speed - speed).abs() > 0.1 {
                            self.to_mpv_send.send(mpv::Request::speed(speed)).await?;
                            self.log.push(LogEntry {
                                when: Instant::now(),
                                what: format!("{} changed speed to {}", from, speed),
                            });
                            self.display_osd().await?;
                        }
                    }
                }
            }
        };

        Ok(())
    }
}
