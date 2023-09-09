use async_std::{
    channel::{unbounded, Receiver, SendError, Sender},
    task,
};
use bevy::prelude::*;
use futures::{future::Either, prelude::*};
use libp2p::{
    core::upgrade,
    dcutr, dns, gossipsub, identify, identity,
    kad::{self, store::MemoryStore, RecordKey},
    noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmBuilder},
    tcp, websocket, yamux, Multiaddr, PeerId, StreamProtocol, Transport,
};
use serde::{Deserialize, Serialize};
use std::thread;

use crate::crypto::DataEncryptor;

const BOOTNODES: [&str; 4] = [
    "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
    "QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
    "QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
    "QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
];

const IDENTIFY_PROTOCOL: &str = "/bevy-p2p-demo/v1";
const RELAY_PROTOCOL: &str = "/libp2p/circuit/relay/0.2.0/hop";

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay: relay::client::Behaviour,
    dcutr: dcutr::Behaviour,
    kad: kad::Kademlia<MemoryStore>,
    gossip: gossipsub::Behaviour<DataEncryptor, gossipsub::AllowAllSubscriptionFilter>,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameEvent<FromGame> {
    Admin(GameAdminEvent),
    Game(FromGame),
}

// For things like killing the swarm and replacing it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameAdminEvent {
    Host { room_code: String },
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Event)]
pub enum NetworkEvent<ToGame> {
    Admin(NetworkAdminEvent),
    Game(ToGame),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkAdminEvent {
    Connected(PeerId),
    Disconnected(PeerId),
    NewNetworkAddress(Multiaddr),
}

#[derive(Resource, Debug, Clone)]
pub struct NetworkManager<FromGame, ToGame> {
    to_network: Sender<GameEvent<FromGame>>,
    from_network: Receiver<NetworkEvent<ToGame>>,
}

impl<FromGame, ToGame> NetworkManager<FromGame, ToGame> {
    pub async fn send_to_network(
        &mut self,
        event: GameEvent<FromGame>,
    ) -> Result<(), SendError<GameEvent<FromGame>>> {
        self.to_network.send(event).await
    }
}

pub async fn setup_network<FromGame, ToGame>(
) -> Result<NetworkManager<FromGame, ToGame>, anyhow::Error>
where
    FromGame: Send + 'static,
    ToGame: Send + 'static,
{
    let id_keys = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(id_keys.public());
    log::info!("Local peer id: {}", local_peer_id);

    let (relay_transport, relay) = relay::client::new(local_peer_id.clone());
    let tcp_transport = dns::DnsConfig::custom(
        tcp::async_io::Transport::new(tcp::Config::default().nodelay(true)),
        dns::ResolverConfig::google(),
        dns::ResolverOpts::default(),
    )
    .await?;

    let ws_transport = websocket::WsConfig::new(
        dns::DnsConfig::custom(
            tcp::async_io::Transport::new(tcp::Config::default().nodelay(true)),
            dns::ResolverConfig::google(),
            dns::ResolverOpts::default(),
        )
        .await?,
    );
    // TODO: quic transport

    let transport = tcp_transport
        .or_transport(ws_transport)
        .or_transport(relay_transport)
        .upgrade(upgrade::Version::V1Lazy)
        .authenticate(noise::Config::new(&id_keys).expect("signing libp2p-noise static keypair"))
        .multiplex(yamux::Config::default())
        .timeout(std::time::Duration::from_secs(20))
        .boxed();

    let behaviour: Behaviour = {
        let mut kad = kad::Kademlia::new(
            local_peer_id.clone(),
            MemoryStore::new(local_peer_id.clone()),
        );
        for peer in &BOOTNODES {
            kad.add_address(&peer.parse()?, "/dnsaddr/bootstrap.libp2p.io".parse()?);
        }
        let config = gossipsub::Config::default();
        let (data_encryptor, aes_keys) = DataEncryptor::new();
        let gossip = gossipsub::Behaviour::new_with_transform(
            gossipsub::MessageAuthenticity::Signed(id_keys.clone()),
            config,
            None,
            data_encryptor,
        )
        .map_err(|s: &str| anyhow::anyhow!(s))?;
        let dcutr = dcutr::Behaviour::new(local_peer_id);
        let ping = ping::Behaviour::default();
        let identify = identify::Behaviour::new(identify::Config::new(
            IDENTIFY_PROTOCOL.into(),
            id_keys.public(),
        ));
        Behaviour {
            relay,
            dcutr,
            kad,
            gossip,
            ping,
            identify,
        }
    };

    let mut swarm =
        SwarmBuilder::with_async_std_executor(transport, behaviour, local_peer_id).build();

    swarm.behaviour_mut().kad.bootstrap()?;

    // Send events over channel.
    let (to_network, from_game): (Sender<GameEvent<FromGame>>, Receiver<GameEvent<FromGame>>) =
        unbounded();
    let (to_game, from_network): (Sender<NetworkEvent<ToGame>>, Receiver<NetworkEvent<ToGame>>) =
        unbounded();

    // Start thread that loops for events and reads the channels
    thread::spawn(move || {
        task::block_on(async {
            let mut to_game = to_game;
            let mut from_game = from_game;
            let mut swarm = swarm;
            loop {
                match futures::future::select(
                    swarm.select_next_some(),
                    from_game.select_next_some(),
                )
                .await
                {
                    Either::Left((event, _)) => match event {
                        libp2p::swarm::SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                            // to_game
                            //     .send(NetworkEvent::Admin(NetworkAdminEvent::Connected(peer_id)))
                            //     .await
                            //     .unwrap();
                        }
                        libp2p::swarm::SwarmEvent::ConnectionClosed { peer_id, .. } => {
                            to_game
                                .send(NetworkEvent::Admin(NetworkAdminEvent::Disconnected(
                                    peer_id,
                                )))
                                .await
                                .unwrap();
                        }
                        libp2p::swarm::SwarmEvent::IncomingConnection { .. } => {}
                        libp2p::swarm::SwarmEvent::IncomingConnectionError { .. } => {}
                        libp2p::swarm::SwarmEvent::OutgoingConnectionError { .. } => {}
                        libp2p::swarm::SwarmEvent::NewListenAddr { address, .. } => {
                            log::info!("New listen addr: {:?}", address);
                        }
                        libp2p::swarm::SwarmEvent::ExpiredListenAddr { .. } => {}
                        libp2p::swarm::SwarmEvent::ListenerClosed { .. } => {}
                        libp2p::swarm::SwarmEvent::ListenerError { .. } => {}
                        libp2p::swarm::SwarmEvent::Dialing { peer_id, .. } => {}
                        libp2p::swarm::SwarmEvent::Behaviour(e) => {
                            handle_behaviour_event(e, &mut to_game).await
                        }
                    },
                    Either::Right((msg, _)) => match msg {
                        GameEvent::Admin(GameAdminEvent::Quit) => break,
                        GameEvent::Admin(GameAdminEvent::Host { room_code }) => {
                            // Start swarm listening.
                            swarm
                                .listen_on(
                                    "/dns4/p2p.favil.org/tcp/4001/p2p/\
                                 12D3KooWJAmx46jdsLbvsEJmUAnQ44Yj4iHmgdsDD4BEYvALnFy8/p2p-circuit"
                                        .parse()
                                        .expect("Parse should always work"),
                                )
                                .expect("Listen should work");
                            swarm
                                .listen_on("/ip4/0.0.0.0/tcp/0".parse().expect("parse"))
                                .expect("Listen should work");
                            swarm
                                .listen_on("/ip4/0.0.0.0/tcp/0/ws".parse().expect("parse"))
                                .expect("Listen should work");
                            swarm
                                .dial(
                                    "/dns4/p2p.favil.org/tcp/4001"
                                        .parse::<Multiaddr>()
                                        .expect("parse"),
                                )
                                .expect("Dial should work");
                            swarm
                                .behaviour_mut()
                                .kad
                                .start_providing(RecordKey::new(
                                    &format!("/bevy-libp2p-demo/room/{}", room_code).as_bytes(),
                                ))
                                .expect("Providing");
                        }
                        GameEvent::Admin(_) => todo!(),
                        GameEvent::Game(_) => todo!(),
                    },
                }
            }
        });
    });

    Ok(NetworkManager {
        from_network,
        to_network,
    })
}

async fn handle_behaviour_event<ToGame>(
    event: BehaviourEvent,
    sender: &mut Sender<NetworkEvent<ToGame>>,
) {
    log::debug!("Behaviour event: {:?}", event);
    match event {
        BehaviourEvent::Identify(identify::Event::Received { peer_id, info }) => {
            if info
                .protocols
                .contains(&StreamProtocol::new(IDENTIFY_PROTOCOL))
            {
                sender
                    .send(NetworkEvent::Admin(NetworkAdminEvent::Connected(peer_id)))
                    .await
                    .unwrap();
            }

            if info
                .protocols
                .contains(&StreamProtocol::new(RELAY_PROTOCOL))
            {
                // log::error!("Peer {} supports relay", peer_id);
            }
        }
        BehaviourEvent::Kad(
            kad::KademliaEvent::OutboundQueryProgressed {
                result: kad::QueryResult::StartProviding(
                    Ok(kad::AddProviderOk {key})
                ),
                    ..
            } 
        ) => {
            log::info!("Started providing for our room: {:?}", key);
        }
        _ => {}
    }
}

pub(crate) struct NetworkPlugin;

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, process_network_events::<(), ()>)
            .add_event::<NetworkEvent<()>>();
    }
}

fn process_network_events<ToGame, FromGame>(
    network_manager: ResMut<NetworkManager<FromGame, ToGame>>,
    mut network_events: EventWriter<NetworkEvent<ToGame>>,
) where
    ToGame: Send + Sync + 'static,
    FromGame: Send + 'static,
{
    while let Ok(event) = network_manager.from_network.try_recv() {
        network_events.send(event);
    }
}
