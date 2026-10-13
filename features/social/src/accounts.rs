use crate::{Result, SocialFeature, wire};
use serde::Deserialize;
use social::*;
use std::sync::Arc;
use storage::social::{SetAdministrator, SetContact, SetContactBlock, UpdateAccount};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactInput {
    pub request_id: CommandId,
    pub listed: bool,
    pub expected_revision: Option<Revision>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactBlockInput {
    pub request_id: CommandId,
    pub blocked: bool,
    pub expected_revision: Option<Revision>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdministratorInput {
    pub request_id: CommandId,
    pub account_id: ActorId,
    pub granted: bool,
    pub expected_revision: Revision,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInput {
    pub request_id: CommandId,
    pub name: String,
    pub bio: String,
    pub expected_revision: Revision,
}

impl SocialFeature {
    pub async fn account(&self, id: ActorId) -> Result<wire::Account> {
        let s = self.services()?;
        Ok(s.resources.store.actor(s.human, id).await?.into())
    }
    pub async fn managed_accounts(&self) -> Result<Vec<wire::ManagedAi>> {
        let s = self.services()?;
        Ok(s.resources
            .store
            .managed_accounts(s.human)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn managed_account(&self, id: ActorId) -> Result<wire::ManagedAi> {
        let s = self.services()?;
        Ok(s.resources.store.managed_account(s.human, id).await?.into())
    }
    pub async fn set_administrator(
        &self,
        id: ActorId,
        input: AdministratorInput,
    ) -> Result<wire::ManagedAi> {
        Ok(self
            .services()?
            .resources
            .store
            .set_administrator(
                self.context(input.request_id)?,
                id,
                SetAdministrator {
                    account_id: input.account_id,
                    granted: input.granted,
                    expected_revision: input.expected_revision,
                },
            )
            .await?
            .into())
    }
    pub async fn set_contact_block(
        self: &Arc<Self>,
        id: ActorId,
        input: ContactBlockInput,
    ) -> Result<ContactEntry> {
        let s = self.services()?;
        let entry = s
            .resources
            .store
            .set_contact_block(
                self.context(input.request_id)?,
                SetContactBlock {
                    account_id: id,
                    blocked: input.blocked,
                    expected_revision: input.expected_revision,
                },
            )
            .await?;
        self.stop_terminal_executions().await?;
        for conversation in s.resources.store.queued_conversations(s.human).await? {
            self.schedule_queue(conversation);
        }
        Ok(entry)
    }
    pub async fn set_contact(&self, id: ActorId, input: ContactInput) -> Result<ContactEntry> {
        self.services()?
            .resources
            .store
            .set_contact(
                self.context(input.request_id)?,
                SetContact {
                    account_id: id,
                    listed: input.listed,
                    expected_revision: input.expected_revision,
                },
            )
            .await
            .map_err(Into::into)
    }
    pub async fn update_account(&self, id: ActorId, input: AccountInput) -> Result<wire::Account> {
        Ok(self
            .services()?
            .resources
            .store
            .update_account(
                self.context(input.request_id)?,
                id,
                UpdateAccount {
                    display_name: input.name,
                    bio: input.bio,
                    expected_revision: input.expected_revision,
                },
            )
            .await?
            .into())
    }
    pub async fn account_view(&self, id: ActorId) -> Result<wire::AccountView> {
        let s = self.services()?;
        let o = s.resources.store.account_view(s.human, id).await?;
        Ok(wire::AccountView {
            operator_id: s.human,
            snapshot: wire::Init {
                self_id: o.account.id,
                self_name: o.account.display_name.clone(),
                self_account: o.account.into(),
                accounts: o.accounts.into_iter().map(Into::into).collect(),
                address_book: o.address_book,
                conversations: o.conversations.into_iter().map(Into::into).collect(),
                models: vec![],
                cursor: o.cursor,
            },
        })
    }
    pub async fn account_view_messages(
        &self,
        id: ActorId,
        conversation: ConversationId,
        before: Option<Sequence>,
    ) -> Result<wire::MessagePage> {
        let s = self.services()?;
        let (messages, has_more) = s
            .resources
            .store
            .account_view_messages(s.human, id, conversation, before)
            .await?;
        Ok(wire::MessagePage {
            read_through: None,
            messages: messages.into_iter().map(Into::into).collect(),
            has_more,
        })
    }
}
