use std::collections::HashSet;

use bevy::prelude::*;
use libp2p::PeerId;

use crate::network::{NetworkAdminEvent, NetworkEvent};

pub struct PeerPlugin;

#[derive(Resource, Debug, Clone, Default)]
struct Peers(HashSet<PeerId>);

impl Plugin for PeerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, peer_add_remove::<()>)
            .insert_resource(Peers::default());
    }
}

fn peer_add_remove<ToGame>(mut event: EventReader<NetworkEvent<ToGame>>, mut peers: ResMut<Peers>)
where
    ToGame: Send + Sync + 'static,
{
    for event in event.iter() {
        match event {
            NetworkEvent::Admin(NetworkAdminEvent::Connected(peer_id)) => {
                log::info!("Peer added: {}", peer_id);
                peers.0.insert(*peer_id);
            }
            NetworkEvent::Admin(NetworkAdminEvent::Disconnected(peer_id)) => {
                if peers.0.contains(peer_id) {
                    log::info!("Peer removed: {}", peer_id);
                    peers.0.remove(peer_id);
                }
            }
            _ => {}
        }
    }
}
