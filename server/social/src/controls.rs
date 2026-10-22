use crate::{Result, SocialFeature, wire};
use social::*;
use std::sync::Arc;
use storage::social::{
    ConversationAction, ConversationControl, EndConversation, PauseMember, ReviseMessage,
    WithdrawMessage,
};

impl SocialFeature {
    pub async fn update_participation(
        self: &Arc<Self>,
        id: ConversationId,
        input: wire::ParticipationInput,
    ) -> Result<wire::Conversation> {
        let s = self.services()?;
        let locks = self.conversation_locks(id);
        let _mutation = locks.mutation.lock().await;
        s.resources
            .store
            .update_participation(
                self.context(input.request_id)?,
                storage::social::UpdateParticipation {
                    conversation_id: id,
                    policy: input.policy,
                },
            )
            .await?;
        self.stop_terminal_executions().await?;
        self.schedule_queue(id);
        self.conversation(id).await
    }
    pub async fn message(&self, id: MessageId) -> Result<wire::Message> {
        let s = self.services()?;
        Ok(s.resources.store.message(s.human, id).await?.into())
    }
    pub async fn revise_message(
        self: &Arc<Self>,
        id: MessageId,
        input: wire::ReviseInput,
    ) -> Result<wire::Message> {
        let s = self.services()?;
        let original = s.resources.store.message(s.human, id).await?;
        let locks = self.conversation_locks(original.conversation_id);
        let _mutation = locks.mutation.lock().await;
        let message = s
            .resources
            .store
            .revise_message(
                self.context(input.request_id)?,
                ReviseMessage {
                    message_id: id,
                    text: input.text,
                    expected_revision: input.expected_revision,
                },
            )
            .await?;
        self.stop_terminal_executions().await?;
        self.schedule_queue(original.conversation_id);
        Ok(message.into())
    }
    pub async fn withdraw_message(
        self: &Arc<Self>,
        id: MessageId,
        input: wire::VersionedCommand,
    ) -> Result<wire::Message> {
        let s = self.services()?;
        let original = s.resources.store.message(s.human, id).await?;
        let locks = self.conversation_locks(original.conversation_id);
        let _mutation = locks.mutation.lock().await;
        let message = s
            .resources
            .store
            .withdraw_message(
                self.context(input.request_id)?,
                WithdrawMessage {
                    message_id: id,
                    expected_revision: input.expected_revision,
                },
            )
            .await?;
        self.stop_terminal_executions().await?;
        self.schedule_queue(original.conversation_id);
        Ok(message.into())
    }
    pub async fn pause_member(
        self: &Arc<Self>,
        id: ConversationId,
        input: wire::PauseMemberInput,
    ) -> Result<wire::Conversation> {
        let s = self.services()?;
        let locks = self.conversation_locks(id);
        let _mutation = locks.mutation.lock().await;
        s.resources
            .store
            .pause_member(
                self.context(input.request_id)?,
                PauseMember {
                    conversation_id: id,
                    actor_id: input.actor_id,
                    expected_revision: input.expected_revision,
                    paused: input.paused,
                },
            )
            .await?;
        self.stop_terminal_executions().await?;
        self.schedule_queue(id);
        self.conversation(id).await
    }
    pub async fn control_conversation(
        self: &Arc<Self>,
        id: ConversationId,
        input: wire::ControlInput,
    ) -> Result<wire::Conversation> {
        let s = self.services()?;
        let locks = self.conversation_locks(id);
        let _mutation = locks.mutation.lock().await;
        match input.action {
            wire::ControlAction::End => {
                s.resources
                    .store
                    .end_conversation(
                        self.context(input.request_id)?,
                        EndConversation {
                            conversation_id: id,
                            expected_revision: input.expected_revision,
                        },
                    )
                    .await?;
            }
            action => {
                let action = match action {
                    wire::ControlAction::Pause => ConversationAction::Pause,
                    wire::ControlAction::Resume => ConversationAction::Resume,
                    wire::ControlAction::Archive => ConversationAction::Archive,
                    wire::ControlAction::Restore => ConversationAction::Restore,
                    wire::ControlAction::End => unreachable!(),
                };
                s.resources
                    .store
                    .control_conversation(
                        self.context(input.request_id)?,
                        ConversationControl {
                            conversation_id: id,
                            expected_revision: input.expected_revision,
                            action,
                        },
                    )
                    .await?;
            }
        }
        self.stop_terminal_executions().await?;
        self.schedule_queue(id);
        self.conversation(id).await
    }
    pub(crate) async fn stop_terminal_executions(&self) -> Result<()> {
        let s = self.services()?;
        let active: Vec<_> = self
            .cancellations
            .lock()
            .await
            .iter()
            .map(|(id, token)| (*id, token.clone()))
            .collect();
        for (id, token) in active {
            if s.resources
                .store
                .run(s.human, id)
                .await?
                .status
                .is_terminal()
            {
                token.cancel();
            }
        }
        Ok(())
    }
}
