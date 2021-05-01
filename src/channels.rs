use crate::mpv;
use crate::p2p;
use crate::statemachine;
use async_channel::{self, Receiver, Sender};

const CHANNEL_SIZE: usize = 256;

pub struct PeerChannels {
    // p2p channels
    pub to_network_receive: Receiver<p2p::Action>,
    pub from_network_send: Sender<(p2p::Peer, p2p::Message)>,
}

pub struct LogicChannels {
    pub from_mpv_receive: Receiver<mpv::Event>,
    pub to_mpv_send: Sender<mpv::Request>,
    pub from_network_receive: Receiver<(p2p::Peer, p2p::Message)>,
    pub to_network_send: Sender<p2p::Action>,
    pub to_logic_send: Sender<statemachine::Event>,
    pub from_logic_receive: Receiver<statemachine::Event>,
}

impl LogicChannels {
    pub fn split(&self) -> LogicSenderChannels {
        LogicSenderChannels {
            to_mpv_send: self.to_mpv_send.clone(),
            to_network_send: self.to_network_send.clone(),
            to_logic_send: self.to_logic_send.clone(),
        }
    }
}

pub struct LogicSenderChannels {
    pub to_mpv_send: Sender<mpv::Request>,
    pub to_network_send: Sender<p2p::Action>,
    pub to_logic_send: Sender<statemachine::Event>,
}

pub struct MpvChannels {
    pub to_mpv_receive: Receiver<mpv::Request>,
    pub from_mpv_send: Sender<mpv::Event>,
}

pub struct SyncoChannels {
    pub p2p: PeerChannels,
    pub logic: LogicChannels,
    pub mpv: MpvChannels,
}

impl SyncoChannels {
    pub fn new_client() -> SyncoChannels {
        let (to_network_send, to_network_receive) = async_channel::bounded(CHANNEL_SIZE);
        let (from_network_send, from_network_receive) = async_channel::bounded(CHANNEL_SIZE);
        let (to_mpv_send, to_mpv_receive) = async_channel::bounded(CHANNEL_SIZE);
        let (from_mpv_send, from_mpv_receive) = async_channel::bounded(CHANNEL_SIZE);
        let (to_logic_send, from_logic_receive) = async_channel::bounded(CHANNEL_SIZE);

        SyncoChannels {
            p2p: PeerChannels {
                to_network_receive,
                from_network_send,
            },
            logic: LogicChannels {
                from_mpv_receive,
                to_mpv_send,
                from_network_receive,
                to_network_send,
                to_logic_send,
                from_logic_receive,
            },
            mpv: MpvChannels {
                from_mpv_send,
                to_mpv_receive,
            },
        }
    }
}
