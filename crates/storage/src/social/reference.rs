use super::{SocialStorageError as Error, SocialStorageResult as Result, sqlite::Db, store::*};
use ::social::*;
use rusqlite::params;

impl Db<'_> {
    pub(super) fn reference_preview(
        &self,
        operator: ActorId,
        actor: ActorId,
        selection: &ReferenceSelection,
    ) -> Result<ReferencePreview> {
        self.operator(operator)?;
        let ai: Actor = self.get("actors", actor)?;
        if ai.kind != ActorKind::Ai {
            return Err(Error::Invalid("私聊接续对象必须是 AI 账号".into()));
        }
        let conversation: Conversation = self.get("conversations", selection.conversation_id)?;
        let mut dependencies = Vec::new();
        for id in [operator, actor] {
            let m = self.member(id, conversation.id)?;
            dependencies.push(VisibilityDependency {
                conversation_id: conversation.id,
                actor_id: id,
                membership_revision: m.revision,
                context_epoch: conversation.context_epoch,
            });
        }
        if conversation.kind != ConversationKind::Group || conversation.legacy {
            return Err(Error::Invalid("接续来源必须是共同群聊".into()));
        }
        if selection.sources.is_empty() || selection.sources.len() > 9 {
            return Err(Error::Invalid(
                "请选择 1–8 条原文，可附带一个相关分段摘要".into(),
            ));
        }
        let mut messages = Vec::new();
        let mut summary = None;
        for source in &selection.sources {
            match source {
                SourceRef::Message {
                    conversation_id,
                    message_id,
                    revision,
                } => {
                    let m: SocialMessage = self.get("messages", message_id)?;
                    if *conversation_id != conversation.id
                        || m.conversation_id != conversation.id
                        || m.status != MessageStatus::Committed
                        || m.revision != *revision
                    {
                        return Err(Error::Conflict(
                            "引用原文已修改、撤回或不属于所选群聊，请重新选择".into(),
                        ));
                    }
                    if messages.iter().any(|old: &SocialMessage| old.id == m.id)
                        || messages.len() == 8
                    {
                        return Err(Error::Invalid("最多引用 8 条不同的群消息".into()));
                    }
                    messages.push(m);
                }
                SourceRef::Summary {
                    summary_id,
                    revision,
                } if summary.is_none() => {
                    self.validate_summary_source(
                        ContextScope {
                            actor_id: actor,
                            conversation_id: conversation.id,
                        },
                        *summary_id,
                        *revision,
                    )?;
                    summary = Some(
                        self.get::<ConversationSummary>("conversation_summaries", summary_id)?,
                    );
                }
                _ => return Err(Error::Invalid("引用只支持原文及一个相关群摘要".into())),
            }
        }
        if messages.is_empty() || summary.as_ref().is_some_and(|s| !s.sources.iter().any(|source| matches!(source, SourceRef::Message { message_id, .. } if messages.iter().any(|m|m.id == *message_id)))) {
            return Err(Error::Invalid("请选中原文；摘要必须覆盖至少一条所选消息".into()));
        }
        messages.sort_by_key(|m| m.sequence);
        Ok(ReferencePreview {
            conversation,
            messages,
            summary,
            dependencies,
        })
    }

    pub(super) fn attach_reference(
        &self,
        message: &mut SocialMessage,
        actor: ActorId,
        selection: ReferenceSelection,
        preview: &ReferencePreview,
    ) -> Result<()> {
        message.context_reference = Some(ContextReference {
            target: ContextScope {
                actor_id: actor,
                conversation_id: message.conversation_id,
            },
            initiated_by: message.sender_id,
            attached_to: message.id,
            selection,
            stale: false,
        });
        let payload = serde_json::to_string(message)?;
        self.execute(
            "UPDATE messages SET payload=?2 WHERE id=?1",
            params![message.id.to_string(), payload],
        )?;
        self.execute(
            "UPDATE message_revisions SET payload=?2 WHERE message_id=?1 AND revision=1",
            params![message.id.to_string(), payload],
        )?;
        self.execute(
            "INSERT INTO context_references VALUES(?1,?2,?3)",
            params![
                message.id.to_string(),
                preview.conversation.id.to_string(),
                actor.to_string()
            ],
        )?;
        let mut ids: Vec<_> = preview.messages.iter().map(|m| m.id).collect();
        if let Some(summary) = &preview.summary {
            ids.extend(summary.sources.iter().filter_map(|source| match source {
                SourceRef::Message { message_id, .. } => Some(*message_id),
                _ => None,
            }));
        }
        let ids: std::collections::HashSet<_> = ids.into_iter().collect();
        for id in ids {
            self.execute(
                "INSERT INTO context_reference_sources VALUES(?1,?2)",
                params![message.id.to_string(), id.to_string()],
            )?;
        }
        Ok(())
    }

    pub(super) fn reference_material(
        &self,
        operator: ActorId,
        scope: ContextScope,
        messages: &[SocialMessage],
        trigger: &SocialMessage,
    ) -> Result<Option<ReferenceMaterial>> {
        let selected = if trigger.context_reference.is_some() {
            Some(trigger)
        } else if let Some(quoted) = trigger.reply_to.and_then(|id| {
            messages
                .iter()
                .find(|m| m.id == id && m.context_reference.is_some())
        }) {
            Some(quoted)
        } else {
            messages
                .iter()
                .filter(|m| m.sequence <= trigger.sequence)
                .rev()
                .take(12)
                .find(|m| m.context_reference.is_some())
        };
        let Some(message) = selected else {
            return Ok(None);
        };
        let reference = message
            .context_reference
            .as_ref()
            .expect("selected reference");
        let required = message.id == trigger.id || Some(message.id) == trigger.reply_to;
        if reference.stale {
            return if required {
                Err(Error::Conflict(
                    "群话题引用已失效，请返回群聊重新选择".into(),
                ))
            } else {
                Ok(None)
            };
        }
        if reference.target != scope || reference.initiated_by != operator {
            return Err(Error::Forbidden("引用不属于当前私聊参与者".into()));
        }
        let preview = self.reference_preview(operator, scope.actor_id, &reference.selection)?;
        Ok(Some(ReferenceMaterial {
            message_id: message.id,
            revision: message.revision,
            required,
            preview,
        }))
    }

    pub(super) fn validate_reference(
        &self,
        operator: ActorId,
        scope: ContextScope,
        id: MessageId,
        revision: Revision,
    ) -> Result<ContextReference> {
        let m: SocialMessage = self.get("messages", id)?;
        let r = m
            .context_reference
            .ok_or_else(|| Error::Invalid("消息没有群话题引用".into()))?;
        if m.status != MessageStatus::Committed || m.revision != revision || r.stale {
            return Err(Error::Conflict("群话题引用已失效".into()));
        }
        if m.conversation_id != scope.conversation_id
            || r.target != scope
            || r.initiated_by != operator
        {
            return Err(Error::Forbidden("引用不属于当前私聊参与者".into()));
        }
        self.reference_preview(operator, scope.actor_id, &r.selection)?;
        Ok(r)
    }

    pub(super) fn stale_references_for_message(&self, id: MessageId) -> Result<()> {
        let messages = self.query_json("SELECT m.payload FROM messages m JOIN context_reference_sources r ON r.reference_message_id=m.id WHERE r.source_message_id=?1", [id.to_string()])?;
        self.stale_references(messages)
    }
    pub(super) fn stale_references_for_member(
        &self,
        conversation: ConversationId,
        actor: ActorId,
    ) -> Result<()> {
        let messages = self.query_json("SELECT m.payload FROM messages m JOIN context_references r ON r.message_id=m.id WHERE r.source_conversation_id=?1 AND (r.actor_id=?2 OR m.sender_id=?2)", params![conversation.to_string(),actor.to_string()])?;
        self.stale_references(messages)
    }
    fn stale_references(&self, messages: Vec<SocialMessage>) -> Result<()> {
        for mut m in messages {
            let r = m.context_reference.as_mut().expect("indexed reference");
            if r.stale || m.status != MessageStatus::Committed {
                continue;
            }
            r.stale = true;
            m.revision = m.revision.next()?;
            self.update_message(&m, m.sender_id)?;
            self.stale_memories(m.id)?;
            self.stale_summaries(m.id)?;
            self.cancel_trigger_plans(m.id)?;
            let mut c: Conversation = self.get("conversations", m.conversation_id)?;
            c.revision = c.revision.next()?;
            c.context_epoch = c.context_epoch.next()?;
            self.invalidate_context(c.id)?;
            self.save_conversation(&c)?;
        }
        Ok(())
    }
}
