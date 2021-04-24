use libp2p::{
    floodsub::{self, Floodsub, FloodsubEvent},
    identity::Keypair,
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    multiaddr,
    noise::NoiseError,
    swarm,
    swarm::NetworkBehaviourEventProcess,
    Multiaddr, NetworkBehaviour, PeerId, Swarm, TransportError,
};

use async_channel::{Receiver, Sender};

// TODO: find a way to replace select! with something
// that is provided by futures-lite
use futures::{
    future::FutureExt, // for `.fuse()`
    pin_mut,
    select,
};

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
    }
}

#[derive(Debug)]
pub struct LogicError {
    reason: String,
}

pub type Message = Vec<u8>;

pub async fn join(
    id_keys: Keypair,
    topic_id: &str,
    control: Receiver<Message>,
    tap: Sender<Message>,
) -> Result<(), JoinError> {
    env_logger::init();

    let peer_id = PeerId::from(id_keys.public());

    println!("Local peer id: {:?}", peer_id);

    // TODO: don't use development transport
    let transport = libp2p::development_transport(id_keys).await?;

    let floodsub_topic = floodsub::Topic::new(topic_id);

    #[derive(NetworkBehaviour)]
    struct SyncoNetworkBehaviour {
        floodsub: Floodsub,
        mdns: Mdns,
    }

    impl NetworkBehaviourEventProcess<FloodsubEvent> for SyncoNetworkBehaviour {
        // Called when `floodsub` produces an event.
        fn inject_event(&mut self, message: FloodsubEvent) {
            if let FloodsubEvent::Message(message) = message {
                println!(
                    "Received: '{:?}' from {:?}",
                    String::from_utf8_lossy(&message.data),
                    message.source
                );
            }
        }
    }

    impl NetworkBehaviourEventProcess<MdnsEvent> for SyncoNetworkBehaviour {
        // Called when `mdns` produces an event.
        fn inject_event(&mut self, event: MdnsEvent) {
            match event {
                MdnsEvent::Discovered(list) => {
                    for (peer, _) in list {
                        self.floodsub.add_node_to_partial_view(peer);
                    }
                }
                MdnsEvent::Expired(list) => {
                    for (peer, _) in list {
                        if !self.mdns.has_node(&peer) {
                            self.floodsub.remove_node_from_partial_view(&peer);
                        }
                    }
                }
            }
        }
    }

    let mut swarm = {
        let mdns = Mdns::new(MdnsConfig::default()).await?;
        let mut behaviour = SyncoNetworkBehaviour {
            floodsub: Floodsub::new(peer_id.clone()),
            mdns,
        };

        behaviour.floodsub.subscribe(floodsub_topic.clone());
        Swarm::new(transport, behaviour, peer_id)
    };

    if let Some(to_dial) = std::env::args().nth(1) {
        let addr: Multiaddr = to_dial.parse()?;
        swarm.dial_addr(addr)?;
        println!("Dialed {:?}", to_dial)
    }

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    swarm.listen_on("/ip6/::/tcp/0".parse()?)?;

    loop {
        enum NextStep {
            Nothing,
            Stop,
            Send(Message),
        }

        match {
            let f1 = control.recv().fuse();
            let f2 = swarm.next_event().fuse();

            pin_mut!(f1, f2);

            select! {
                msg = f1 => match msg {
                    Ok(msg) => NextStep::Send(msg),
                    Err(err) => {
                        println!("Stopping due to control error: {:?}", err);
                        NextStep::Stop
                    },
                },
                event = f2 => {
                    println!("Got swarm event: {:?}", event);
                    NextStep::Nothing
                },
            }
        } {
            NextStep::Nothing => continue,
            NextStep::Stop => {
                println!("Unsubscribing from the topic");
                swarm
                    .behaviour_mut()
                    .floodsub
                    .unsubscribe(floodsub_topic.clone());
                return Ok(());
            }
            NextStep::Send(msg) => {
                swarm
                    .behaviour_mut()
                    .floodsub
                    .publish(floodsub_topic.clone(), msg);
            }
        };
    }
}
