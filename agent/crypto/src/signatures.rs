//! Application metadata signatures use SDK signing and Matrix canonical JSON.
//! Megolm remains the encryption protocol; these signatures preserve sender
//! authentication when historical inbound sessions are restored from backup.
use crate::Device;
use anyhow::{Result, ensure};
use matrix_sdk_crypto::{
    UserIdentity,
    vodozemac::{Ed25519PublicKey, Ed25519Signature},
};
use serde_json::{Value, json};
fn canonical(envelope: &Value) -> Result<String> {
    let mut body = envelope.clone();
    body.as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("invalid sealed envelope"))?
        .remove("signatures");
    Ok(serde_json::to_string(
        &ruma::canonical_json::to_canonical_value(body)?,
    )?)
}
impl Device {
    pub async fn sign_envelope(&self, envelope: &mut Value) -> Result<()> {
        ensure!(
            envelope["sender"] == self.machine().user_id().as_str()
                && envelope["deviceId"] == self.machine().device_id().as_str(),
            "sealed sender does not match signing device"
        );
        let keys = self.machine().identity_keys();
        envelope["deviceKeys"] =
            json!({"ed25519":keys.ed25519.to_base64(),"curve25519":keys.curve25519.to_base64()});
        let message = canonical(envelope)?;
        envelope["signatures"] = serde_json::to_value(self.machine().sign(&message).await?)?;
        self.verify_envelope(envelope).await
    }
    pub async fn verify_envelope(&self, envelope: &Value) -> Result<()> {
        let sender = ruma::UserId::parse(
            envelope["sender"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("sealed sender missing"))?,
        )?;
        let identity = self
            .machine()
            .get_identity(&sender, None)
            .await?
            .ok_or_else(|| anyhow::anyhow!("sender identity unavailable"))?;
        ensure!(
            identity.is_verified(),
            "sender master identity is not verified"
        );
        let master = match identity {
            UserIdentity::Own(i) => i.master_key().get_first_key(),
            UserIdentity::Other(i) => i.master_key().get_first_key(),
        }
        .ok_or_else(|| anyhow::anyhow!("sender master key missing"))?;
        let device = envelope["deviceId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("sealed device missing"))?;
        ensure!(
            envelope["content"]["device_id"] == device
                && envelope["content"]["sender_key"] == envelope["deviceKeys"]["curve25519"],
            "sealed encryption device binding mismatch"
        );
        let message = canonical(envelope)?;
        let signatures = &envelope["signatures"][sender.as_str()];
        let master_id = format!("ed25519:{}", master.to_base64());
        let master_signature = Ed25519Signature::from_base64(
            signatures[&master_id]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("master signature missing"))?,
        )?;
        master.verify(message.as_bytes(), &master_signature)?;
        let device_key = Ed25519PublicKey::from_base64(
            envelope["deviceKeys"]["ed25519"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("signed device key missing"))?,
        )?;
        let device_signature = Ed25519Signature::from_base64(
            signatures[format!("ed25519:{device}")]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("device signature missing"))?,
        )?;
        device_key.verify(message.as_bytes(), &device_signature)?;
        Ok(())
    }
}
