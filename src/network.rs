use async_std::{
    channel::{unbounded, Receiver, Sender},
    stream::StreamExt,
};
use bevy::prelude::*;
use futures::{future::Either, prelude::*};
use libp2p::{
    core::upgrade,
    dns, gossipsub, identify, identity,dcutr,
    kad::{self, store::MemoryStore},
    noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmBuilder},
    tcp, websocket, yamux, Multiaddr, PeerId, Transport,
};
use serde::{Deserialize, Serialize};
use std::thread;

use crate::crypto::DataEncryptor;

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
pub enum GameAdminEvent {}

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

pub async fn setup_network<FromGame, ToGame>(
) -> Result<NetworkManager<FromGame, ToGame>, anyhow::Error>
where
    FromGame: Send + 'static,
    ToGame: Send + 'static,
{
    let id_keys = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(id_keys.public());

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
        let kad = kad::Kademlia::new(
            local_peer_id.clone(),
            MemoryStore::new(local_peer_id.clone()),
        );
        let config = gossipsub::Config::default();
        let (data_encryptor, aes_keys) = DataEncryptor::new();
        let gossip = gossipsub::Behaviour::new_with_transform(
            gossipsub::MessageAuthenticity::Signed(id_keys.clone()),
            config,
            None,
            data_encryptor,
        )
        .map_err(|s: &str| anyhow::anyhow!(s))?;
        let ping = ping::Behaviour::default();
        let identify = identify::Behaviour::new(id_keys.public().clone());
        Behaviour {
            relay,
            kad,
            gossip,
            ping,
            identify,
        }
    };

    let mut swarm =
        SwarmBuilder::with_async_std_executor(transport, behaviour, local_peer_id).build();

    // Start swarm listening.
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    swarm.listen_on("/ip4/0.0.0.0/tcp/0/ws".parse()?)?;

    // Send events over channel.
    let (to_network, from_game): (Sender<GameEvent<FromGame>>, Receiver<GameEvent<FromGame>>) =
        unbounded();
    let (to_game, from_network): (Sender<NetworkEvent<ToGame>>, Receiver<NetworkEvent<ToGame>>) =
        unbounded();

    // Start thread that loops for events and reads the channels
    thread::spawn(move || async {
        let mut to_game = to_game;
        let mut from_game = from_game;
        let mut swarm = swarm;
        loop {
            match futures::future::select(swarm.select_next_some(), from_game.select_next_some())
                .await
            {
                Either::Left((event, _)) => match event {
                    libp2p::swarm::SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        to_game
                            .send(NetworkEvent::Admin(NetworkAdminEvent::Connected(peer_id)))
                            .await
                            .unwrap();
                    }
                    libp2p::swarm::SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        to_game
                            .send(NetworkEvent::Admin(NetworkAdminEvent::Disconnected(
                                peer_id,
                            )))
                            .await
                            .unwrap();
                    }
                    libp2p::swarm::SwarmEvent::IncomingConnection {
                        connection_id,
                        local_addr,
                        send_back_addr,
                    } => {}
                    libp2p::swarm::SwarmEvent::IncomingConnectionError {
                        connection_id,
                        local_addr,
                        send_back_addr,
                        error,
                    } => {}
                    libp2p::swarm::SwarmEvent::OutgoingConnectionError {
                        connection_id,
                        peer_id,
                        error,
                    } => {}
                    libp2p::swarm::SwarmEvent::NewListenAddr {
                        listener_id,
                        address,
                    } => {}
                    libp2p::swarm::SwarmEvent::ExpiredListenAddr {
                        listener_id,
                        address,
                    } => {}
                    libp2p::swarm::SwarmEvent::ListenerClosed {
                        listener_id,
                        addresses,
                        reason,
                    } => {}
                    libp2p::swarm::SwarmEvent::ListenerError { listener_id, error } => {}
                    libp2p::swarm::SwarmEvent::Dialing {
                        peer_id,
                        connection_id,
                    } => {}
                    libp2p::swarm::SwarmEvent::Behaviour(e) => {
                        handle_behaviour_event(e, &mut to_game)
                    }
                },
                Either::Right((msg, _)) => match msg {
                    GameEvent::Admin(_) => todo!(),
                    GameEvent::Game(_) => todo!(),
                },
            }
        }
    });

    Ok(NetworkManager {
        from_network,
        to_network,
    })
}

fn handle_behaviour_event<ToGame>(
    event: BehaviourEvent,
    sender: &mut Sender<NetworkEvent<ToGame>>,
) {
    todo!()
}

pub(crate) struct NetworkPlugin;

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, process_network_events);
    }
}

fn process_network_events(
    network_manager: Res<NetworkManager<(), ()>>,
    network_events: EventWriter<NetworkEvent<()>>,
) {
}
