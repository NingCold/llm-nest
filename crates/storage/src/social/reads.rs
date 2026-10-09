use super::{SocialStorageError as Error, SocialStorageResult as Result, sqlite::Db, store::*};
use ::social::*;
use rusqlite::params;

impl Db<'_> {
    pub(super) fn contact_record(&self, id: ActorId) -> Result<ContactRecord> {
        let actor: Actor = self.get("actors", id)?;
        let profile = self
            .query_json(
                "SELECT payload FROM ai_profiles WHERE actor_id=?1",
                [id.to_string()],
            )?
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound(format!("AI profile/{id}")))?;
        let binding = self
            .query_json(
                "SELECT payload FROM execution_bindings WHERE actor_id=?1",
                [id.to_string()],
            )?
            .into_iter()
            .next();
        Ok(ContactRecord {
            actor,
            profile,
            binding,
        })
    }

    pub(super) fn conversation_record(
        &self,
        operator: ActorId,
        conversation: Conversation,
    ) -> Result<ConversationRecord> {
        let member = self.member(operator, conversation.id)?;
        let members = self.query_json("SELECT payload FROM memberships WHERE conversation_id=?1 AND status='active' ORDER BY actor_id", [conversation.id.to_string()])?;
        let latest_message = self.query_json("SELECT payload FROM messages WHERE conversation_id=?1 AND sequence>=?2 ORDER BY sequence DESC LIMIT 1", params![conversation.id.to_string(), member.history_from_seq.get() as i64])?.into_iter().next();
        let active_run = self
            .query_json(
                "SELECT payload FROM ai_runs WHERE conversation_id=?1 AND status='running'",
                [conversation.id.to_string()],
            )?
            .into_iter()
            .next();
        Ok(ConversationRecord {
            conversation,
            members,
            latest_message,
            active_run,
        })
    }

    pub(super) fn overview(&self, operator: ActorId) -> Result<SocialOverview> {
        let human = self.operator(operator)?;
        let actors: Vec<Actor> = self.query_json(
            "SELECT payload FROM actors WHERE kind='ai' AND status='active' ORDER BY rowid",
            [],
        )?;
        let contacts = actors
            .into_iter()
            .map(|actor| self.contact_record(actor.id))
            .collect::<Result<_>>()?;
        let conversations = self.query_json("SELECT c.payload FROM conversations c JOIN memberships m ON m.conversation_id=c.id WHERE m.actor_id=?1 AND m.status='active' ORDER BY c.rowid DESC", [operator.to_string()])?
            .into_iter().map(|c| self.conversation_record(operator, c)).collect::<Result<_>>()?;
        let cursor: i64 = self
            .connection
            .query_row(
                "SELECT coalesce(max(cursor),0) FROM committed_events",
                [],
                |r| r.get(0),
            )
            .map_err(|e| super::schema::sql_error(self.path, e))?;
        Ok(SocialOverview {
            human,
            contacts,
            conversations,
            cursor: EventCursor::new(cursor as u64)?,
        })
    }

    pub(super) fn context_material(
        &self,
        operator: ActorId,
        scope: ContextScope,
        trigger: MessageId,
    ) -> Result<ContextMaterial> {
        self.operator(operator)?;
        let conversation: Conversation = self.get("conversations", scope.conversation_id)?;
        let human = self.member(operator, conversation.id)?;
        let ai = self.member(scope.actor_id, conversation.id)?;
        let trigger: SocialMessage = self.get("messages", trigger)?;
        if trigger.conversation_id != conversation.id
            || trigger.sender_id != operator
            || trigger.status != MessageStatus::Committed
        {
            return Err(Error::Invalid(
                "trigger must be a committed local message in this conversation".into(),
            ));
        }
        let visible_from = human.history_from_seq.max(ai.history_from_seq);
        let mut messages: Vec<SocialMessage> = self.query_json("SELECT payload FROM messages WHERE conversation_id=?1 AND status='committed' AND sequence>=?2 AND sequence<=?3 ORDER BY sequence DESC LIMIT 200", params![conversation.id.to_string(), visible_from.get() as i64, trigger.sequence.get() as i64])?;
        if messages.first().map(|m| m.id) != Some(trigger.id) {
            return Err(Error::Forbidden(
                "trigger is outside the visible history".into(),
            ));
        }
        messages.reverse();
        let relationship_revision = conversation
            .relationship_id
            .map(|id| {
                self.get::<Relationship>("relationships", id)
                    .map(|r| r.revision)
            })
            .transpose()?;
        Ok(ContextMaterial {
            contact: self.contact_record(scope.actor_id)?,
            conversation,
            members: vec![human, ai],
            relationship_revision,
            messages,
        })
    }

    pub(super) fn authorized_run(&self, operator: ActorId, id: RunId) -> Result<AiRun> {
        self.operator(operator)?;
        let run: AiRun = self.get("ai_runs", id)?;
        self.member(operator, run.snapshot.scope.conversation_id)?;
        Ok(run)
    }

    pub(super) fn save_details(&self, id: RunId, details: &RunDetails) -> Result<()> {
        self.execute("INSERT INTO run_details(run_id,payload) VALUES(?1,?2) ON CONFLICT(run_id) DO UPDATE SET payload=excluded.payload", params![id.to_string(),serde_json::to_string(details)?])?;
        Ok(())
    }
}
