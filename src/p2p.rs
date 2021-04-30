use libp2p::{
    core::muxing::StreamMuxerBox,
    core::transport,
    core::transport::upgrade::Version,
    identity::{self, Keypair},
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    multiaddr,
    noise::{self, NoiseError},
    ping::{self, Ping, PingConfig, PingEvent},
    swarm,
    swarm::NetworkBehaviourEventProcess,
    tcp::TcpConfig,
    yamux::YamuxConfig,
    Multiaddr, NetworkBehaviour, PeerId, Swarm, Transport, TransportError,
};

use libp2p::gossipsub::{
    self, error::PublishError, error::SubscriptionError, Gossipsub, GossipsubEvent,
    GossipsubMessage, IdentTopic as Topic, MessageAuthenticity, MessageId, ValidationMode,
};

use crate::proto;
use async_channel::{Receiver, Sender};
use log::debug;
use log::error;
use log::info;
use log::warn;
use smol::future;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

const PING_INTERVAL: Duration = Duration::from_secs(2);

pub type Message = proto::Message;
pub type Action = proto::Action;

quick_error! {
    #[derive(Debug)]
    pub enum JoinError {
        IOError(err: std::io::Error) {
            from()
        }
        LogicError(err: LogicError) {
            from()
        }
        NoiseError(err: NoiseError) {
            from()
        }
        SwarmDialError(err: swarm::DialError) {
            from()
        }
        MultiaddrError(err: multiaddr::Error) {
            from()
        }
        IOTransportError(err: TransportError<std::io::Error>) {
            from()
        }
        JSONError(err: serde_json::Error) {
            from()
        }
        SubscriptionError(err: SubscriptionError) {
            from()
        }
        PublishError(err: PublishError) {
            from()
        }
    }
}

#[derive(Debug)]
pub struct LogicError {
    reason: String,
}

pub type Peer = String;

pub fn build_transport(
    key_pair: identity::Keypair,
) -> Result<transport::Boxed<(PeerId, StreamMuxerBox)>, JoinError> {
    let noise_keys = noise::Keypair::<noise::X25519Spec>::new().into_authentic(&key_pair)?;
    let noise_config = noise::NoiseConfig::xx(noise_keys).into_authenticated();
    let yamux_config = YamuxConfig::default();
    let base_transport = TcpConfig::new().nodelay(true);

    Ok(base_transport
        .upgrade(Version::V1)
        .authenticate(noise_config)
        .multiplex(yamux_config)
        // .timeout(Duration::from_secs(60))
        .boxed())
}

pub async fn join(
    id_keys: Keypair,
    user_id: &str,
    topic_id: &str,
    control: Receiver<Action>,
    tap: Sender<(Peer, Message)>,
) -> Result<(), JoinError> {
    env_logger::init();

    let peer_id = PeerId::from(id_keys.public());

    info!("Local peer id: {:?}", peer_id);

    let transport = build_transport(id_keys.clone())?;

    let topic = Topic::new(topic_id);

    #[derive(NetworkBehaviour)]
    struct SyncoNetworkBehaviour {
        gossipsub: Gossipsub,
        mdns: Mdns,
        ping: Ping,

        #[behaviour(ignore)]
        tap: Arc<Sender<(Peer, Message)>>,
    }

    impl NetworkBehaviourEventProcess<PingEvent> for SyncoNetworkBehaviour {
        // Called when `ping` produces an event.
        fn inject_event(&mut self, event: PingEvent) {
            use ping::handler::{PingFailure, PingSuccess};
            match event {
                PingEvent {
                    peer,
                    result: Result::Ok(PingSuccess::Ping { rtt }),
                } => {
                    debug!(
                        "ping: rtt to {} is {} ms",
                        peer.to_base58(),
                        rtt.as_millis()
                    );
                }
                PingEvent {
                    peer,
                    result: Result::Ok(PingSuccess::Pong),
                } => {
                    debug!("ping: pong from {}", peer.to_base58());
                }
                PingEvent {
                    peer,
                    result: Result::Err(PingFailure::Timeout),
                } => {
                    debug!("ping: timeout to {}", peer.to_base58());
                }
                PingEvent {
                    peer,
                    result: Result::Err(PingFailure::Other { error }),
                } => {
                    debug!("ping: failure with {}: {}", peer.to_base58(), error);
                }
            }
        }
    }

    impl NetworkBehaviourEventProcess<GossipsubEvent> for SyncoNetworkBehaviour {
        fn inject_event(&mut self, message: GossipsubEvent) {
            if let GossipsubEvent::Message {
                message,
                propagation_source,
                ..
            } = message
            {
                debug!(
                    "peer {:?} sent message: {:?}",
                    message.source,
                    String::from_utf8_lossy(&message.data),
                );

                #[cfg(not(feature = "relay"))]
                {
                    // I wonder how ugly is too much ugly
                    let tap = self.tap.clone();

                    smol::spawn(async move {
                        let msg: proto::Message =
                            serde_json::from_slice(message.data.as_slice()).unwrap(); // TODO unwrap

                        tap.send((propagation_source.to_base58(), msg))
                            .await
                            .unwrap_or_else(|e| {
                                error!("p2p: error while sending message to tap: {}", e);
                            })
                    })
                    // TODO: this is also not perfect
                    // messages may appear out of order
                    // (well, they may appear out of order for too many reasons...)
                    .detach();
                };
            }
        }
    }

    impl NetworkBehaviourEventProcess<MdnsEvent> for SyncoNetworkBehaviour {
        // Called when `mdns` produces an event.
        fn inject_event(&mut self, event: MdnsEvent) {
            match event {
                MdnsEvent::Discovered(list) => {
                    for (peer, _) in list {
                        warn!("mdns: discovered peer {}", peer);
                        self.gossipsub.add_explicit_peer(&peer);
                    }
                }
                MdnsEvent::Expired(list) => {
                    for (peer, _) in list {
                        if !self.mdns.has_node(&peer) {
                            warn!("mdns: removing peer {}", peer);
                            self.gossipsub.remove_explicit_peer(&peer);
                        }
                    }
                }
            }
        }
    }

    let mut swarm = {
        let mdns = Mdns::new(MdnsConfig::default()).await?;
        let ping = Ping::new(
            PingConfig::new()
                .with_interval(PING_INTERVAL)
                .with_keep_alive(true),
        );

        let message_id_fn = |message: &GossipsubMessage| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            MessageId::from(s.finish().to_string())
        };

        let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10))
            .validation_mode(ValidationMode::Strict)
            .message_id_fn(message_id_fn)
            .build()
            .expect("Valid config"); // TODO

        let gossipsub = Gossipsub::new(
            MessageAuthenticity::Signed(id_keys.clone()),
            gossipsub_config,
        )
        .expect("Gossipsub creation failed"); // TODO

        let mut behaviour = SyncoNetworkBehaviour {
            gossipsub,
            mdns,
            ping,
            tap: Arc::new(tap),
        };

        // #[cfg(not(feature = "relay"))]
        behaviour.gossipsub.subscribe(&topic)?;

        Swarm::new(transport, behaviour, peer_id)
    };

    if let Ok(to_dial) = std::env::var("SYNCO_RELAY") {
        let addr: Multiaddr = to_dial.parse()?;
        swarm.dial_addr(addr)?;
        info!("manual_dial: dialed {:?}", to_dial)
    }

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    // introduce lots of network issues
    // but make revan satisfied
    swarm.listen_on("/ip6/::/tcp/0".parse()?)?;

    loop {
        enum NextStep {
            Nothing,
            Stop,
            Send(Message),
        }

        match {
            async fn consume_control(user_id: String, control: Receiver<Action>) -> NextStep {
                match control.recv().await {
                    Ok(action) => {
                        let t = SystemTime::now();
                        let ts = t.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();

                        NextStep::Send(proto::Message {
                            ts,
                            action,
                            user_id,
                        })
                    }
                    Err(err) => {
                        error!("Stopping due to control error: {:?}", err);
                        NextStep::Stop
                    }
                }
            }

            async fn swarm_future_wrap<F: future::Future>(f: F) -> NextStep
            where
                <F as smol::future::Future>::Output: std::fmt::Debug,
            {
                smol::pin!(f);
                let ev = f.await;

                info!("Got swarm event: {:?}", ev);
                NextStep::Nothing
            }

            future::or(
                consume_control(user_id.clone().to_string(), control.clone()),
                swarm_future_wrap(swarm.next_event()),
            )
            .await
        } {
            NextStep::Nothing => continue,
            NextStep::Stop => {
                info!("unsubscribing from the topic (pre-shutdown)");
                swarm.behaviour_mut().gossipsub.unsubscribe(&topic)?;
                return Ok(());
            }
            NextStep::Send(msg) => {
                #[cfg(not(feature = "relay"))]
                {
                    // TODO unwrap
                    let encoded = serde_json::to_vec(&msg).unwrap();

                    debug!("send data: {:?}", msg);

                    match swarm
                        .behaviour_mut()
                        .gossipsub
                        .publish(topic.clone(), encoded)
                    {
                        Ok(msgid) => info!("published new message: {}", msgid),
                        Err(err) => error!("could not publish the message: {:?}", err),
                    }
                }
            }
        };
    }
}
