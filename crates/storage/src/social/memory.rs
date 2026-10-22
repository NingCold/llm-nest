use super::{SocialStorageError as Error, SocialStorageResult as Result, sqlite::Db, store::*};
use ::social::*;
use connector::SocialChange;
use rusqlite::params;

impl Db<'_> {
    pub(super) fn memories(
        &self,
        operator: ActorId,
        scope: ContextScope,
    ) -> Result<Vec<MemoryRecord>> {
        self.operator(operator)?;
        self.member(operator, scope.conversation_id)?;
        self.query_json("SELECT payload FROM memories WHERE conversation_id=?1 AND owner_actor_id=?2 AND status!='forgotten' ORDER BY updated_at DESC,id", params![scope.conversation_id.to_string(), scope.actor_id.to_string()])
    }

    pub(super) fn authorized_memory(
        &self,
        operator: ActorId,
        id: MemoryId,
    ) -> Result<MemoryRecord> {
        self.operator(operator)?;
        let memory: MemoryRecord = self.get("memories", id)?;
        self.member(operator, memory.conversation_id)?;
        Ok(memory)
    }

    fn memory_scope(
        &self,
        operator: ActorId,
        scope: ContextScope,
    ) -> Result<(Conversation, MemoryScope)> {
        self.member(operator, scope.conversation_id)?;
        self.member(scope.actor_id, scope.conversation_id)?;
        let actor: Actor = self.get("actors", scope.actor_id)?;
        if actor.kind != ActorKind::Ai {
            return Err(Error::Invalid("记忆归属必须是 AI 账号".into()));
        }
        let c: Conversation = self.get("conversations", scope.conversation_id)?;
        self.writable(&c)?;
        let scope = match c.relationship_id {
            Some(relationship_id) => MemoryScope::Relationship { relationship_id },
            None => MemoryScope::Conversation {
                conversation_id: c.id,
            },
        };
        Ok((c, scope))
    }

    fn memory_sources(
        &self,
        operator: ActorId,
        conversation: ConversationId,
        revision: Revision,
        content: &MemoryContent,
    ) -> Result<Vec<SourceRef>> {
        if content.text.trim().is_empty() || content.text.chars().count() > 2000 {
            return Err(Error::Invalid("记忆内容应为 1–2000 个字符".into()));
        }
        let mut sources = vec![SourceRef::Manual {
            author_id: operator,
            revision,
        }];
        if let Some(source) = &content.source {
            let message: SocialMessage = self.get("messages", source.message_id)?;
            if message.conversation_id != conversation {
                return Err(Error::Forbidden("记忆来源必须属于当前聊天".into()));
            }
            if message.status != MessageStatus::Committed || message.revision != source.revision {
                return Err(Error::Conflict("来源已修改或撤回，请核对后重新保存".into()));
            }
            sources.push(SourceRef::Message {
                conversation_id: conversation,
                message_id: message.id,
                revision: message.revision,
            });
        }
        Ok(sources)
    }

    fn save_memory(&self, memory: &MemoryRecord) -> Result<()> {
        let status = match memory.status {
            MaterialStatus::Confirmed => "confirmed",
            MaterialStatus::Stale => "stale",
            MaterialStatus::Forgotten => "forgotten",
            MaterialStatus::Candidate => unreachable!("explicit memory cannot be a candidate"),
        };
        let payload = serde_json::to_string(memory)?;
        self.execute("INSERT INTO memories VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET status=excluded.status,revision=excluded.revision,updated_at=excluded.updated_at,payload=excluded.payload", params![memory.id.to_string(), memory.conversation_id.to_string(), memory.owner_actor_id.to_string(), status, memory.revision.get() as i64, memory.updated_at.to_rfc3339(), payload])?;
        self.execute(
            "INSERT INTO memory_revisions VALUES(?1,?2,?3)",
            params![memory.id.to_string(), memory.revision.get() as i64, payload],
        )?;
        self.execute(
            "DELETE FROM memory_message_sources WHERE memory_id=?1",
            [memory.id.to_string()],
        )?;
        for source in &memory.sources {
            if let SourceRef::Message {
                message_id,
                revision,
                ..
            } = source
            {
                self.execute(
                    "INSERT INTO memory_message_sources VALUES(?1,?2,?3)",
                    params![
                        memory.id.to_string(),
                        message_id.to_string(),
                        revision.get() as i64
                    ],
                )?;
            }
        }
        self.event(SocialChange::MemoryChanged {
            conversation_id: memory.conversation_id,
            actor_id: memory.owner_actor_id,
            memory_id: memory.id,
            revision: memory.revision,
        })?;
        Ok(())
    }

    pub(super) fn create_memory(
        &self,
        ctx: &CommandContext,
        input: &CreateMemory,
    ) -> Result<MemoryRecord> {
        if let Some(id) = self.receipt(ctx, "create_memory", input)? {
            return self.authorized_memory(
                ctx.operator,
                id.parse()
                    .map_err(|_| Error::Invalid("invalid memory receipt".into()))?,
            );
        }
        let (mut c, scope) = self.memory_scope(ctx.operator, input.scope)?;
        let sources = self.memory_sources(ctx.operator, c.id, Revision::ONE, &input.content)?;
        let memory = MemoryRecord {
            id: MemoryId::new(),
            owner_actor_id: input.scope.actor_id,
            conversation_id: c.id,
            scope,
            kind: input.content.kind,
            text: input.content.text.trim().into(),
            status: MaterialStatus::Confirmed,
            revision: Revision::ONE,
            sources,
            created_by: ctx.operator,
            updated_by: ctx.operator,
            created_at: self.now,
            updated_at: self.now,
        };
        self.save_memory(&memory)?;
        // New entries apply to subsequent runs; already-started replies keep their snapshot.
        c.revision = c.revision.next()?;
        self.save_conversation(&c)?;
        self.save_receipt(ctx, "create_memory", input, memory.id)?;
        Ok(memory)
    }

    pub(super) fn update_memory(
        &self,
        ctx: &CommandContext,
        input: &UpdateMemory,
    ) -> Result<MemoryRecord> {
        if self.receipt(ctx, "update_memory", input)?.is_some() {
            return self.authorized_memory(ctx.operator, input.id);
        }
        let mut memory = self.authorized_memory(ctx.operator, input.id)?;
        self.expected(memory.revision, input.expected_revision)?;
        if memory.status == MaterialStatus::Forgotten {
            return Err(Error::Conflict(
                "记忆已遗忘；如需重新记住，请明确创建新条目".into(),
            ));
        }
        let (mut c, _) = self.memory_scope(
            ctx.operator,
            ContextScope {
                actor_id: memory.owner_actor_id,
                conversation_id: memory.conversation_id,
            },
        )?;
        memory.revision = memory.revision.next()?;
        memory.sources =
            self.memory_sources(ctx.operator, c.id, memory.revision, &input.content)?;
        memory.kind = input.content.kind;
        memory.text = input.content.text.trim().into();
        memory.status = MaterialStatus::Confirmed;
        memory.updated_by = ctx.operator;
        memory.updated_at = self.now;
        self.save_memory(&memory)?;
        self.memory_context_changed(&mut c)?;
        self.save_receipt(ctx, "update_memory", input, memory.id)?;
        Ok(memory)
    }

    pub(super) fn forget_memory(
        &self,
        ctx: &CommandContext,
        id: MemoryId,
        expected_revision: Revision,
    ) -> Result<MemoryRecord> {
        let input = (id, expected_revision);
        if self.receipt(ctx, "forget_memory", &input)?.is_some() {
            return self.authorized_memory(ctx.operator, id);
        }
        let mut memory = self.authorized_memory(ctx.operator, id)?;
        self.expected(memory.revision, expected_revision)?;
        if memory.status != MaterialStatus::Forgotten {
            memory.status = MaterialStatus::Forgotten;
            memory.text.clear();
            memory.revision = memory.revision.next()?;
            memory.updated_by = ctx.operator;
            memory.updated_at = self.now;
            self.save_memory(&memory)?;
            self.execute("UPDATE memory_revisions SET payload=json_set(payload,'$.text','') WHERE memory_id=?1", [id.to_string()])?;
            let mut c = self.get("conversations", memory.conversation_id)?;
            self.memory_context_changed(&mut c)?;
        }
        self.save_receipt(ctx, "forget_memory", &input, id)?;
        Ok(memory)
    }

    fn memory_context_changed(&self, conversation: &mut Conversation) -> Result<()> {
        conversation.context_epoch = conversation.context_epoch.next()?;
        conversation.revision = conversation.revision.next()?;
        self.invalidate_context(conversation.id)?;
        self.save_conversation(conversation)
    }

    pub(super) fn stale_memories(&self, source: MessageId) -> Result<()> {
        for mut memory in self.query_json::<MemoryRecord>("SELECT m.payload FROM memories m JOIN memory_message_sources s ON s.memory_id=m.id WHERE s.message_id=?1 AND m.status='confirmed'", [source.to_string()])? {
            memory.status = MaterialStatus::Stale;
            memory.revision = memory.revision.next()?;
            memory.updated_at = self.now;
            self.save_memory(&memory)?;
        }
        Ok(())
    }

    pub(super) fn validate_memory_source(
        &self,
        scope: ContextScope,
        id: MemoryId,
        revision: Revision,
    ) -> Result<()> {
        let memory: MemoryRecord = self.get("memories", id)?;
        if memory.owner_actor_id != scope.actor_id
            || memory.conversation_id != scope.conversation_id
        {
            return Err(Error::Forbidden("记忆不属于当前 AI 与聊天范围".into()));
        }
        if memory.status != MaterialStatus::Confirmed || memory.revision != revision {
            return Err(Error::Conflict("记忆已修改、失效或遗忘".into()));
        }
        for source in &memory.sources {
            if let SourceRef::Message {
                message_id,
                revision,
                ..
            } = source
            {
                let message: SocialMessage = self.get("messages", message_id)?;
                if message.status != MessageStatus::Committed || message.revision != *revision {
                    return Err(Error::Conflict("记忆来源已变化".into()));
                }
            }
        }
        Ok(())
    }
}
