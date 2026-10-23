//! Official Matrix state machine backed by its durable SQLite crypto store.
//! Callers must persist encrypted payloads before transmission; retrying an
//! application command never calls encryption again.
use anyhow::Result;
pub use matrix_sdk_crypto;
use matrix_sdk_crypto::{OlmMachine, OlmMachineBuilder};
use matrix_sdk_sqlite::SqliteCryptoStore;
pub use ruma;
use std::io::Read;
use std::path::Path;

pub struct Device {
    machine: OlmMachine,
    store: SqliteCryptoStore,
}
impl Device {
    /// `store_key` is device-local and must not accompany cloud recovery data.
    pub async fn open(
        directory: &Path,
        account: &str,
        device: &str,
        store_key: &[u8],
    ) -> Result<Self> {
        let user = ruma::UserId::parse(format!("@{account}:llmn"))?;
        let store = SqliteCryptoStore::open_with_key(directory, Some(store_key)).await?;
        let machine = OlmMachineBuilder::new(&user, device.into())
            .with_crypto_store(store.clone())
            .build()
            .await?;
        Ok(Self { machine, store })
    }
    pub fn machine(&self) -> &OlmMachine {
        &self.machine
    }
    pub async fn receive_sync(&self, response: serde_json::Value) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct ToDevice {
            events: Vec<ruma::serde::Raw<ruma::events::AnyToDeviceEvent>>,
        }
        #[derive(serde::Deserialize)]
        struct Sync {
            next_batch: String,
            to_device: ToDevice,
            device_lists: ruma::api::client::sync::sync_events::DeviceLists,
            device_one_time_keys_count:
                std::collections::BTreeMap<ruma::OneTimeKeyAlgorithm, ruma::UInt>,
            device_unused_fallback_key_types: Option<Vec<ruma::OneTimeKeyAlgorithm>>,
        }
        let response: Sync = serde_json::from_value(response)?;
        self.machine
            .receive_sync_changes(
                matrix_sdk_crypto::EncryptionSyncChanges {
                    to_device_events: response.to_device.events,
                    changed_devices: &response.device_lists,
                    one_time_keys_counts: &response.device_one_time_keys_count,
                    unused_fallback_keys: response.device_unused_fallback_key_types.as_deref(),
                    next_batch_token: Some(response.next_batch.clone()),
                },
                &matrix_sdk_crypto::DecryptionSettings {
                    sender_device_trust_requirement:
                        matrix_sdk_crypto::TrustRequirement::CrossSigned,
                },
            )
            .await?;
        Ok(response.next_batch)
    }
    /// Same-device transport for ordinary SDK Olm events. It never advances a
    /// server mailbox cursor and retains the same strict sender verification.
    pub async fn receive_local_events(&self, events: Vec<serde_json::Value>) -> Result<()> {
        let events = events
            .into_iter()
            .map(|event| ruma::serde::Raw::new(&event).map(|raw| raw.cast_unchecked()))
            .collect::<Result<Vec<_>, _>>()?;
        self.machine
            .receive_sync_changes(
                matrix_sdk_crypto::EncryptionSyncChanges {
                    to_device_events: events,
                    changed_devices: &Default::default(),
                    one_time_keys_counts: &Default::default(),
                    unused_fallback_keys: None,
                    next_batch_token: None,
                },
                &matrix_sdk_crypto::DecryptionSettings {
                    sender_device_trust_requirement:
                        matrix_sdk_crypto::TrustRequirement::CrossSigned,
                },
            )
            .await?;
        Ok(())
    }
    pub async fn open_envelope(&self, envelope: &serde_json::Value) -> Result<serde_json::Value> {
        self.verify_envelope(envelope).await?;
        let room = ruma::RoomId::parse(
            envelope["roomId"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("sealed room missing"))?,
        )?;
        let event = serde_json::json!({"type":"m.room.encrypted","room_id":envelope["roomId"],"sender":envelope["sender"],"event_id":envelope["eventId"],"origin_server_ts":0,"content":envelope["content"]});
        let raw = ruma::serde::Raw::new(&event)?.cast_unchecked();
        // Sender authenticity was mandatory and independently checked above.
        // Imported Megolm sessions deliberately lack original Olm sender proof.
        let decrypted = self
            .machine
            .decrypt_room_event(
                &raw,
                &room,
                &matrix_sdk_crypto::DecryptionSettings {
                    sender_device_trust_requirement: matrix_sdk_crypto::TrustRequirement::Untrusted,
                },
            )
            .await?;
        Ok(serde_json::from_str(decrypted.event.json().get())?)
    }
    /// Encrypt the SDK SecretsBundle with Matrix's attachment format. The info
    /// is the user's offline recovery secret and must never reach the server.
    pub async fn export_recovery(
        &self,
    ) -> Result<(Vec<u8>, matrix_sdk_crypto::MediaEncryptionInfo)> {
        let bundle = self.machine.store().export_secrets_bundle().await?;
        let mut input = std::io::Cursor::new(serde_json::to_vec(&bundle)?);
        let mut encryptor = matrix_sdk_crypto::AttachmentEncryptor::new(&mut input);
        let mut ciphertext = Vec::new();
        encryptor.read_to_end(&mut ciphertext)?;
        let info = encryptor.finish();
        Ok((ciphertext, info))
    }
    pub async fn import_recovery(
        &self,
        ciphertext: &[u8],
        info: matrix_sdk_crypto::MediaEncryptionInfo,
    ) -> Result<()> {
        let mut input = std::io::Cursor::new(ciphertext);
        let mut decryptor = matrix_sdk_crypto::AttachmentDecryptor::new(&mut input, info)?;
        let mut plaintext = Vec::new();
        decryptor.read_to_end(&mut plaintext)?;
        let bundle = serde_json::from_slice(&plaintext)?;
        self.machine.store().import_secrets_bundle(&bundle).await?;
        Ok(())
    }
    /// Only call after independent trusted-device authorization. Downloading a
    /// server device list or authenticating a node credential does not authorize it.
    pub async fn trust_device(
        &self,
        account: &str,
        device_id: &str,
        ed25519: &str,
        curve25519: &str,
    ) -> Result<()> {
        let user = ruma::UserId::parse(format!("@{account}:llmn"))?;
        let device = self
            .machine
            .get_device(&user, device_id.into(), None)
            .await?
            .ok_or_else(|| anyhow::anyhow!("device keys have not been downloaded"))?;
        anyhow::ensure!(
            device
                .ed25519_key()
                .is_some_and(|key| key.to_base64() == ed25519)
                && device
                    .curve25519_key()
                    .is_some_and(|key| key.to_base64() == curve25519),
            "trusted device fingerprint changed"
        );
        device
            .set_local_trust(matrix_sdk_crypto::LocalTrust::Verified)
            .await?;
        Ok(())
    }
    pub async fn seal(
        &self,
        scope: &str,
        event_type: &str,
        plaintext: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let room = ruma::RoomId::parse(format!("!{scope}:llmn"))?;
        use matrix_sdk_crypto::store::CryptoStore;
        anyhow::ensure!(
            self.store
                .get_outbound_group_session(&room)
                .await?
                .is_some(),
            "room encryption session has not been prepared"
        );
        let raw = ruma::serde::Raw::new(plaintext)?.cast_unchecked();
        Ok(serde_json::to_value(
            self.machine
                .encrypt_room_event_raw(&room, event_type, &raw)
                .await?
                .content,
        )?)
    }
    /// History export contains inbound room keys, never an active device session.
    pub async fn export_history(&self, passphrase: &str) -> Result<String> {
        let keys = self.machine.store().export_room_keys(|_| true).await?;
        Ok(matrix_sdk_crypto::encrypt_room_key_export(
            &keys, passphrase, 500_000,
        )?)
    }
    pub async fn import_history(&self, encrypted: &str, passphrase: &str) -> Result<()> {
        let keys = matrix_sdk_crypto::decrypt_room_key_export(encrypted.as_bytes(), passphrase)?;
        self.machine
            .store()
            .import_exported_room_keys(keys, |_, _| {})
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn recovery_attachment_restores_secrets_and_rejects_tampering() {
        let first_dir = tempfile::tempdir().unwrap();
        let first = Device::open(first_dir.path(), "fixture", "FIRST", &[1; 32])
            .await
            .unwrap();
        first
            .machine()
            .bootstrap_cross_signing(false)
            .await
            .unwrap();
        let backup = matrix_sdk_crypto::store::types::BackupDecryptionKey::from_bytes(&[9; 32]);
        first
            .machine()
            .backup_machine()
            .save_decryption_key(Some(backup), Some("1".into()))
            .await
            .unwrap();
        let (ciphertext, info) = first.export_recovery().await.unwrap();
        let next_dir = tempfile::tempdir().unwrap();
        let next = Device::open(next_dir.path(), "fixture", "SECOND", &[2; 32])
            .await
            .unwrap();
        next.import_recovery(
            &ciphertext,
            serde_json::from_value(serde_json::to_value(&info).unwrap()).unwrap(),
        )
        .await
        .unwrap();
        assert!(next.machine().cross_signing_status().await.is_complete());
        let mut corrupted = ciphertext;
        corrupted[0] ^= 1;
        assert!(next.import_recovery(&corrupted, info).await.is_err());
    }
    #[tokio::test]
    async fn signed_envelope_authenticates_restored_history_without_old_device_lookup() {
        let first_dir = tempfile::tempdir().unwrap();
        let first = Device::open(first_dir.path(), "fixture", "FIRST", &[1; 32])
            .await
            .unwrap();
        first
            .machine()
            .bootstrap_cross_signing(false)
            .await
            .unwrap();
        let room = ruma::RoomId::parse("!room:llmn").unwrap();
        first
            .machine()
            .share_room_key(
                &room,
                std::iter::empty::<&ruma::UserId>(),
                matrix_sdk_crypto::EncryptionSettings::default(),
            )
            .await
            .unwrap();
        let mut envelope = serde_json::json!({"version":1,"roomId":"!room:llmn","sender":"@fixture:llmn","deviceId":"FIRST","eventId":"$message.1.final","content":first.seal("room","com.llmn.content.v1",&serde_json::json!({"text":"recovered secret"})).await.unwrap()});
        first.sign_envelope(&mut envelope).await.unwrap();
        let backup = matrix_sdk_crypto::store::types::BackupDecryptionKey::from_bytes(&[9; 32]);
        first
            .machine()
            .backup_machine()
            .save_decryption_key(Some(backup), Some("1".into()))
            .await
            .unwrap();
        let (ciphertext, info) = first.export_recovery().await.unwrap();
        let history = first
            .export_history("independent recovery password")
            .await
            .unwrap();
        let next_dir = tempfile::tempdir().unwrap();
        let next = Device::open(next_dir.path(), "fixture", "SECOND", &[2; 32])
            .await
            .unwrap();
        next.import_recovery(&ciphertext, info).await.unwrap();
        next.import_history(&history, "independent recovery password")
            .await
            .unwrap();
        let decrypted = next.open_envelope(&envelope).await.unwrap();
        assert_eq!(decrypted["content"]["text"], "recovered secret");
        let mut tampered = envelope.clone();
        tampered["roomId"] = serde_json::json!("!other:llmn");
        assert!(next.open_envelope(&tampered).await.is_err());
        let mut unsigned = envelope.clone();
        unsigned.as_object_mut().unwrap().remove("signatures");
        assert!(next.open_envelope(&unsigned).await.is_err());
        let mut substitute = envelope;
        substitute["deviceKeys"]["ed25519"] =
            serde_json::json!(next.machine().identity_keys().ed25519.to_base64());
        assert!(next.open_envelope(&substitute).await.is_err());
    }
    #[tokio::test]
    async fn reopen_preserves_device_identity() {
        let dir = tempfile::tempdir().unwrap();
        let first = Device::open(dir.path(), "fixture", "DEVICE", &[7; 32])
            .await
            .unwrap();
        let keys = first.machine().identity_keys();
        drop(first);
        let second = Device::open(dir.path(), "fixture", "DEVICE", &[7; 32])
            .await
            .unwrap();
        assert_eq!(keys, second.machine().identity_keys());
    }

    #[tokio::test]
    async fn recovery_queries_restore_previously_signed_peer_trust_without_trusting_strangers() {
        use serde_json::{Value, json};
        async fn query(device: &Device, response: Value) {
            let user = ruma::UserId::parse("@peer:llmn").unwrap();
            device
                .machine()
                .update_tracked_users([user.as_ref()])
                .await
                .unwrap();
            device
                .machine()
                .mark_all_tracked_users_as_dirty()
                .await
                .unwrap();
            let request = device
                .outgoing()
                .await
                .unwrap()
                .into_iter()
                .find(|r| r.kind == "keysQuery")
                .unwrap();
            device.acknowledge(&request, response).await.unwrap();
        }
        let owner_dir = tempfile::tempdir().unwrap();
        let owner = Device::open(owner_dir.path(), "owner", "OLD", &[1; 32])
            .await
            .unwrap();
        owner
            .machine()
            .bootstrap_cross_signing(false)
            .await
            .unwrap();
        let peer_dir = tempfile::tempdir().unwrap();
        let peer = Device::open(peer_dir.path(), "peer", "PEER", &[2; 32])
            .await
            .unwrap();
        let peer_bootstrap = peer.machine().bootstrap_cross_signing(false).await.unwrap();
        let signing = peer_bootstrap.upload_signing_keys_req;
        let mut response = json!({"device_keys":{},"master_keys":{"@peer:llmn":signing.master_key},"self_signing_keys":{"@peer:llmn":signing.self_signing_key},"user_signing_keys":{},"failures":{}});
        query(&owner, response.clone()).await;
        let user = ruma::UserId::parse("@peer:llmn").unwrap();
        let identity = owner
            .machine()
            .get_identity(&user, None)
            .await
            .unwrap()
            .unwrap();
        assert!(!identity.is_verified());
        let matrix_sdk_crypto::UserIdentity::Other(identity) = identity else {
            panic!("peer must be another account")
        };
        let signatures =
            serde_json::to_value(identity.verify().await.unwrap().signed_keys).unwrap();
        response["master_keys"]["@peer:llmn"] = signatures["@peer:llmn"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone();
        let backup = matrix_sdk_crypto::store::types::BackupDecryptionKey::from_bytes(&[9; 32]);
        owner
            .machine()
            .backup_machine()
            .save_decryption_key(Some(backup), Some("1".into()))
            .await
            .unwrap();
        let (ciphertext, info) = owner.export_recovery().await.unwrap();
        let restored_dir = tempfile::tempdir().unwrap();
        let restored = Device::open(restored_dir.path(), "owner", "NEW", &[3; 32])
            .await
            .unwrap();
        restored.import_recovery(&ciphertext, info).await.unwrap();
        assert!(
            restored
                .machine()
                .get_identity(&user, None)
                .await
                .unwrap()
                .is_none()
        );
        query(&restored, response.clone()).await;
        assert!(
            restored
                .machine()
                .get_identity(&user, None)
                .await
                .unwrap()
                .unwrap()
                .is_verified()
        );
        // A server-provided new master has no signature from the restored owner.
        let stranger_dir = tempfile::tempdir().unwrap();
        let stranger = Device::open(stranger_dir.path(), "peer", "IMPOSTOR", &[4; 32])
            .await
            .unwrap();
        let replacement = stranger
            .machine()
            .bootstrap_cross_signing(false)
            .await
            .unwrap()
            .upload_signing_keys_req;
        response["master_keys"]["@peer:llmn"] =
            serde_json::to_value(replacement.master_key).unwrap();
        response["self_signing_keys"]["@peer:llmn"] =
            serde_json::to_value(replacement.self_signing_key).unwrap();
        query(&restored, response).await;
        assert!(
            !restored
                .machine()
                .get_identity(&user, None)
                .await
                .unwrap()
                .unwrap()
                .is_verified()
        );
    }
}
pub mod requests;
pub mod signatures;
