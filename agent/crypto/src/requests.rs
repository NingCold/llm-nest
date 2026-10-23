//! Lossless adapter between official Matrix requests and the platform relay.
use crate::Device;
use anyhow::{Result, bail};
use matrix_sdk_crypto::types::requests::{AnyOutgoingRequest, OutgoingRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub device_id: String,
    pub request_id: String,
    pub kind: String,
    pub body: Value,
}
impl Device {
    pub async fn outgoing(&self) -> Result<Vec<Request>> {
        self.machine()
            .outgoing_requests()
            .await?
            .iter()
            .map(|r| adapt(self.machine().device_id().as_str(), r))
            .collect()
    }
    pub async fn acknowledge(&self, request: &Request, response: Value) -> Result<()> {
        use ruma::api::client::{backup, keys, to_device};
        let id: ruma::OwnedTransactionId = request.request_id.clone().into();
        macro_rules! accept {
            ($t:ty) => {{
                let response = parse::<$t>(&response)?;
                self.machine().mark_request_as_sent(&id, &response).await?;
            }};
        }
        match request.kind.as_str() {
            "keysUpload" => accept!(keys::upload_keys::v3::Response),
            "keysQuery" => accept!(keys::get_keys::v3::Response),
            "keysClaim" => accept!(keys::claim_keys::v3::Response),
            "toDevice" => accept!(to_device::send_event_to_device::v3::Response),
            "signatures" => accept!(keys::upload_signatures::v3::Response),
            "signingKeys" => {
                let response = parse::<keys::upload_signing_keys::v3::Response>(&response)?;
                self.machine()
                    .mark_request_as_sent(
                        &id,
                        matrix_sdk_crypto::types::requests::AnyIncomingResponse::SigningKeysUpload(
                            &response,
                        ),
                    )
                    .await?;
            }
            "keysBackup" => accept!(backup::add_backup_keys::v3::Response),
            _ => bail!("unsupported Matrix request kind"),
        }
        Ok(())
    }
}
pub fn adapt(device: &str, request: &OutgoingRequest) -> Result<Request> {
    let (kind, body) = match request.request() {
        AnyOutgoingRequest::KeysUpload(r) => {
            let mut body = serde_json::Map::new();
            if let Some(keys) = &r.device_keys {
                body.insert("device_keys".into(), serde_json::to_value(keys)?);
            }
            if !r.one_time_keys.is_empty() {
                body.insert(
                    "one_time_keys".into(),
                    serde_json::to_value(&r.one_time_keys)?,
                );
            }
            if !r.fallback_keys.is_empty() {
                body.insert(
                    "fallback_keys".into(),
                    serde_json::to_value(&r.fallback_keys)?,
                );
            }
            ("keysUpload", Value::Object(body))
        }
        AnyOutgoingRequest::KeysQuery(r) => {
            let mut body = json!({"device_keys":r.device_keys});
            if let Some(timeout) = r.timeout {
                body["timeout"] = json!(timeout.as_millis() as u64);
            }
            ("keysQuery", body)
        }
        AnyOutgoingRequest::KeysClaim(r) => {
            let mut body = json!({"one_time_keys":r.one_time_keys});
            if let Some(timeout) = r.timeout {
                body["timeout"] = json!(timeout.as_millis() as u64);
            }
            ("keysClaim", body)
        }
        AnyOutgoingRequest::ToDeviceRequest(r) => ("toDevice", serde_json::to_value(r)?),
        AnyOutgoingRequest::SignatureUpload(r) => {
            ("signatures", serde_json::to_value(&r.signed_keys)?)
        }
        AnyOutgoingRequest::RoomMessage(_) => {
            bail!("in-room verification requires the application message path")
        }
    };
    Ok(Request {
        device_id: device.into(),
        request_id: request.request_id().to_string(),
        kind: kind.into(),
        body,
    })
}

fn parse<T: ruma::api::IncomingResponse>(value: &Value) -> Result<T> {
    let bytes = serde_json::to_vec(value)?;
    Ok(T::try_from_http_response_inner(
        http::Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .body(bytes.as_slice())?,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruma::{api::client::keys::upload_keys::v3, serde::Raw};

    #[test]
    fn replenishing_keys_omits_absent_device_and_fallback_keys() {
        let mut upload = v3::Request::new();
        upload.one_time_keys.insert(
            "signed_curve25519:one-time".try_into().unwrap(),
            Raw::from_json_string(r#"{"key":"opaque-one-time-key"}"#.into()).unwrap(),
        );
        let request = adapt("device", &upload.into()).unwrap();
        assert_eq!(request.kind, "keysUpload");
        assert_eq!(
            request.body,
            json!({"one_time_keys":{"signed_curve25519:one-time":{"key":"opaque-one-time-key"}}})
        );
    }

    #[test]
    fn empty_keys_upload_has_no_optional_fields() {
        let request = adapt("device", &v3::Request::new().into()).unwrap();
        assert_eq!(request.body, json!({}));
    }
}
