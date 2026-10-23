use super::{NodeClient, NodeError};
use crate::crypto::{
    ApplyResource, CryptoRequest, Resource, ResourceList, ResourceParticipants, SaveResource,
};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

impl NodeClient {
    /// Current encrypted windows, available before task materials are ready.
    pub async fn crypto_conversations(
        &self,
    ) -> Result<Vec<crate::node::NodeConversation>, NodeError> {
        #[derive(serde::Deserialize)]
        struct Windows {
            conversations: Vec<crate::node::NodeConversation>,
        }
        let response: Windows = self
            .crypto_get(&["node", "crypto", "conversations"], &[])
            .await?;
        Ok(response.conversations)
    }
    pub fn origin(&self) -> String {
        self.base.origin().ascii_serialization()
    }

    async fn crypto_get<T: DeserializeOwned>(
        &self,
        path: &[&str],
        query: &[(&str, &str)],
    ) -> Result<T, NodeError> {
        let mut url = self.endpoint(path);
        url.query_pairs_mut().extend_pairs(query.iter().copied());
        let response = self.http.get(url).send().await?;
        if !response.status().is_success() {
            return Err(NodeError::Api {
                status: response.status().as_u16(),
                body: response.text().await?,
            });
        }
        Ok(response.json().await?)
    }

    pub async fn crypto_register(&self, device: &str, name: &str) -> Result<Value, NodeError> {
        self.request(
            Method::POST,
            &["crypto", "devices"],
            Some(&json!({"deviceId":device,"name":name})),
        )
        .await
    }

    pub async fn crypto_devices(&self, account: &str) -> Result<Value, NodeError> {
        self.crypto_get(&["crypto", "devices"], &[("accountId", account)])
            .await
    }

    pub async fn crypto_request(&self, body: &CryptoRequest) -> Result<Value, NodeError> {
        self.request(Method::POST, &["crypto", "requests"], Some(body))
            .await
    }

    pub async fn crypto_sync(&self, device: &str, after: &str) -> Result<Value, NodeError> {
        self.crypto_get(
            &["crypto", "sync"],
            &[("deviceId", device), ("after", after)],
        )
        .await
    }

    pub async fn crypto_backup(&self) -> Result<Value, NodeError> {
        self.crypto_get(&["crypto", "backup"], &[]).await
    }

    pub async fn crypto_save_backup(&self, body: &Value) -> Result<Value, NodeError> {
        self.request(Method::PUT, &["crypto", "backup"], Some(body))
            .await
    }

    pub async fn crypto_backup_keys(&self) -> Result<Value, NodeError> {
        self.crypto_get(&["crypto", "backup", "keys"], &[]).await
    }

    pub async fn crypto_recovery(&self) -> Result<Value, NodeError> {
        self.crypto_get(&["crypto", "recovery"], &[]).await
    }

    pub async fn crypto_save_recovery(&self, body: &Value) -> Result<Value, NodeError> {
        self.request(Method::PUT, &["crypto", "recovery"], Some(body))
            .await
    }

    pub async fn crypto_resources(
        &self,
        ai: &str,
        conversation: Option<&str>,
        kind: &str,
    ) -> Result<ResourceList, NodeError> {
        let mut query = vec![("aiId", ai), ("kind", kind)];
        if let Some(conversation) = conversation {
            query.push(("conversationId", conversation));
        }
        self.crypto_get(&["crypto", "resources"], &query).await
    }

    pub async fn crypto_resource_participants(
        &self,
        ai: &str,
        conversation: Option<&str>,
        kind: &str,
    ) -> Result<ResourceParticipants, NodeError> {
        let mut query = vec![("aiId", ai), ("kind", kind)];
        if let Some(conversation) = conversation {
            query.push(("conversationId", conversation));
        }
        self.crypto_get(&["crypto", "resource-participants"], &query)
            .await
    }

    pub async fn crypto_save_resource(
        &self,
        id: &str,
        body: &SaveResource,
    ) -> Result<Resource, NodeError> {
        self.request(Method::PUT, &["crypto", "resources", id], Some(body))
            .await
    }

    pub async fn crypto_apply_resource(
        &self,
        id: &str,
        body: &ApplyResource,
    ) -> Result<Resource, NodeError> {
        self.request(
            Method::POST,
            &["crypto", "resources", id, "applied"],
            Some(body),
        )
        .await
    }

    pub async fn crypto_handover_ready(
        &self,
        device: &str,
        binding: &str,
        backup: &str,
    ) -> Result<Value, NodeError> {
        self.request(
            Method::POST,
            &["node", "handover-ready"],
            Some(&json!({
                "deviceId":device,"expectedBindingVersion":binding,"backupVersion":backup,
            })),
        )
        .await
    }
}
