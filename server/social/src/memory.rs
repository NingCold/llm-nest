use crate::{Result, SocialFeature, wire};
use social::{ContextScope, MemoryId};
use std::sync::Arc;
use storage::social::{CreateMemory, UpdateMemory};

impl SocialFeature {
    pub async fn memories(&self, scope: ContextScope) -> Result<Vec<wire::Memory>> {
        let s = self.services()?;
        Ok(s.resources
            .store
            .memories(s.human, scope)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn create_memory(
        self: &Arc<Self>,
        scope: ContextScope,
        input: wire::CreateMemoryInput,
    ) -> Result<wire::Memory> {
        let s = self.services()?;
        let locks = self.conversation_locks(scope.conversation_id);
        let _mutation = locks.mutation.lock().await;
        Ok(s.resources
            .store
            .create_memory(
                self.context(input.request_id)?,
                CreateMemory {
                    scope,
                    content: input.content,
                },
            )
            .await?
            .into())
    }

    pub async fn update_memory(
        self: &Arc<Self>,
        id: MemoryId,
        input: wire::UpdateMemoryInput,
    ) -> Result<wire::Memory> {
        let s = self.services()?;
        let original = s.resources.store.memory(s.human, id).await?;
        let locks = self.conversation_locks(original.conversation_id);
        let _mutation = locks.mutation.lock().await;
        let memory = s
            .resources
            .store
            .update_memory(
                self.context(input.request_id)?,
                UpdateMemory {
                    id,
                    expected_revision: input.expected_revision,
                    content: input.content,
                },
            )
            .await?;
        self.stop_terminal_executions().await?;
        self.schedule_queue(original.conversation_id);
        Ok(memory.into())
    }

    pub async fn forget_memory(
        self: &Arc<Self>,
        id: MemoryId,
        input: wire::VersionedCommand,
    ) -> Result<wire::Memory> {
        let s = self.services()?;
        let original = s.resources.store.memory(s.human, id).await?;
        let locks = self.conversation_locks(original.conversation_id);
        let _mutation = locks.mutation.lock().await;
        let memory = s
            .resources
            .store
            .forget_memory(self.context(input.request_id)?, id, input.expected_revision)
            .await?;
        self.stop_terminal_executions().await?;
        self.schedule_queue(original.conversation_id);
        Ok(memory.into())
    }
}
