use crate::{Result, SocialFeature, wire};
use social::*;
use std::sync::Arc;
use storage::social::ContactControl;

impl SocialFeature {
    pub async fn control_contact(
        self: &Arc<Self>,
        id: ActorId,
        input: wire::ContactControlInput,
    ) -> Result<wire::Contact> {
        let s = self.services()?;
        let contact = s
            .resources
            .store
            .control_contact(
                self.context(input.request_id)?,
                ContactControl {
                    actor_id: id,
                    expected_revision: input.expected_revision,
                    action: input.action,
                },
            )
            .await?;
        self.stop_terminal_executions().await?;
        // Archiving one member releases its queue position; other AIs still respond.
        for conversation in s.resources.store.queued_conversations(s.human).await? {
            self.schedule_queue(conversation);
        }
        Ok(contact.into())
    }
}
