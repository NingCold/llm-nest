use crate::{Result, SocialFeature, wire};
use social::*;
use std::sync::Arc;
use storage::social::{
    CancelContactIntent, ContactScheduleView, CreateContactIntent, UpdateContactSettings,
};

impl SocialFeature {
    pub async fn update_contact_settings(
        self: &Arc<Self>,
        id: ConversationId,
        input: wire::ContactSettingsInput,
    ) -> Result<ContactScheduleView> {
        let s = self.services()?;
        let locks = self.conversation_locks(id);
        let _mutation = locks.mutation.lock().await;
        let view = s
            .resources
            .store
            .update_contact_settings(
                self.context(input.request_id)?,
                UpdateContactSettings {
                    conversation_id: id,
                    settings: input.settings,
                },
            )
            .await?;
        self.stop_terminal_executions().await?;
        self.schedule_queue(id);
        Ok(view)
    }
    pub async fn create_contact_intent(
        self: &Arc<Self>,
        id: ConversationId,
        input: wire::ContactIntentInput,
    ) -> Result<ContactIntent> {
        let s = self.services()?;
        let locks = self.conversation_locks(id);
        let _mutation = locks.mutation.lock().await;
        let intent = s
            .resources
            .store
            .create_contact_intent(
                self.context(input.request_id)?,
                CreateContactIntent {
                    conversation_id: id,
                    actor_id: input.actor_id,
                    text: input.text,
                    due_at: input.due_at,
                    expires_at: input.expires_at,
                },
            )
            .await?;
        self.schedule_queue(id);
        Ok(intent)
    }
    pub async fn cancel_contact_intent(
        self: &Arc<Self>,
        id: ConversationId,
        intent: IntentId,
        input: wire::VersionedCommand,
    ) -> Result<ContactIntent> {
        let s = self.services()?;
        let locks = self.conversation_locks(id);
        let _mutation = locks.mutation.lock().await;
        let intent = s
            .resources
            .store
            .cancel_contact_intent(
                self.context(input.request_id)?,
                CancelContactIntent {
                    conversation_id: id,
                    intent_id: intent,
                    expected_revision: input.expected_revision,
                },
            )
            .await?;
        self.stop_terminal_executions().await?;
        self.schedule_queue(id);
        Ok(intent)
    }
}
