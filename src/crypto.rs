use std::sync::{Arc, RwLock};

use aes_gcm::{
    aead::{Aead, AeadCore, OsRng, Payload},
    Aes256Gcm, KeyInit,
};
use generic_array::typenum::Unsigned;
use libp2p::gossipsub::DataTransform;

pub struct KeyRing(Arc<RwLock<Vec<Aes256Gcm>>>);

pub struct DataEncryptor {
    keys: KeyRing,
}

impl DataEncryptor {
    pub fn new() -> (Self, KeyRing) {
        let keys = KeyRing(Arc::new(RwLock::new(vec![Aes256Gcm::new(
            &Aes256Gcm::generate_key(OsRng),
        )])));
        (Self { keys: keys.clone() }, keys)
    }
}

impl Clone for KeyRing {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl KeyRing {
    pub fn add_key(
        &mut self,
        key: generic_array::GenericArray<u8, <Aes256Gcm as aes_gcm::KeySizeUser>::KeySize>,
    ) {
        self.0.write().unwrap().push(Aes256Gcm::new(&key));
    }
}

const AAD: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];

impl DataTransform for DataEncryptor {
    fn inbound_transform(
        &self,
        raw_message: libp2p::gossipsub::RawMessage,
    ) -> Result<libp2p::gossipsub::Message, std::io::Error> {
        let data_size = raw_message.data.len() - <Aes256Gcm as AeadCore>::NonceSize::to_usize();
        let nonce = &raw_message.data[data_size..];

        // TODO: try all keys in vec
        let data = self
            .keys
            .0
            .read()
            .expect("key read lock poisoned")
            .iter()
            .rev()
            .find_map(|key| {
                let payload = Payload {
                    msg: &raw_message.data[..data_size],
                    aad: &AAD,
                };
                key.decrypt(nonce.into(), payload).ok()
            })
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Encryption failed: No corresponding key",
            ))?;
        Ok(libp2p::gossipsub::Message {
            data,
            source: raw_message.source,
            sequence_number: raw_message.sequence_number,
            topic: raw_message.topic,
        })
    }

    fn outbound_transform(
        &self,
        _topic: &libp2p::gossipsub::TopicHash,
        data: Vec<u8>,
    ) -> Result<Vec<u8>, std::io::Error> {
        let payload = Payload {
            msg: data.as_ref(),
            aad: &AAD,
        };
        let nonce = Aes256Gcm::generate_nonce(OsRng);
        let mut data = self
            .keys
            .0
            .read()
            .expect("key read lock poisoned")
            .last()
            .unwrap()
            .encrypt(&nonce, payload)
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Decryption failed: {}", e),
                )
            })?;
        data.extend(nonce.as_slice());
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_works() {
        let (encryptor, _keys) = DataEncryptor::new();
        let data = b"Hello, world!";
        let encrypted = encryptor
            .outbound_transform(
                &libp2p::gossipsub::TopicHash::from_raw("test"),
                data.to_vec(),
            )
            .unwrap();
        let raw_message = libp2p::gossipsub::RawMessage {
            data: encrypted,
            source: None,
            sequence_number: Some(0),
            topic: libp2p::gossipsub::TopicHash::from_raw("test"),
            key: None,
            signature: None,
            validated: true,
        };
        let decrypted_msg = encryptor.inbound_transform(raw_message).unwrap();
        assert_eq!(decrypted_msg.data, data);
    }
}
