use crate::{Result, SocialFeature, wire};
use social::*;
use std::sync::Arc;
use storage::social::AccountControl;

impl SocialFeature {
    pub async fn control_account(
        self: &Arc<Self>,
        id: ActorId,
        input: wire::AccountControlInput,
    ) -> Result<wire::Account> {
        let s = self.services()?;
        let contact = s
            .resources
            .store
            .control_account(
                self.context(input.request_id)?,
                AccountControl {
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
        Ok(contact.actor.into())
    }
}
