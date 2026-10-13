use super::{SocialStorageError as Error, SocialStorageResult as Result, sqlite::Db, store::*};
use ::social::*;
use rusqlite::params;

impl Db<'_> {
    pub(super) fn message_page_for(
        &self,
        viewer: ActorId,
        conversation: ConversationId,
        before: Option<Sequence>,
    ) -> Result<(Vec<SocialMessage>, bool)> {
        self.member(viewer, conversation)?;
        let mut messages: Vec<SocialMessage> = self.query_json("SELECT payload FROM messages WHERE conversation_id=?1 AND (?2 IS NULL OR sequence<?2) ORDER BY sequence DESC LIMIT 201", params![conversation.to_string(), before.map(|s|s.get() as i64)])?;
        let more = messages.len() > 200;
        messages.truncate(200);
        messages.reverse();
        Ok((messages, more))
    }

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
        self.member(operator, conversation.id)?;
        let members = self.query_json("SELECT payload FROM memberships WHERE conversation_id=?1 AND status='active' ORDER BY actor_id", [conversation.id.to_string()])?;
        let latest_message = self.query_json("SELECT payload FROM messages WHERE conversation_id=?1 ORDER BY sequence DESC LIMIT 1", [conversation.id.to_string()])?.into_iter().next();
        let active_run = self
            .query_json(
                "SELECT payload FROM ai_runs WHERE conversation_id=?1 AND status='running'",
                [conversation.id.to_string()],
            )?
            .into_iter()
            .next();
        let pending_plans = self.query_json("SELECT payload FROM participation_plans WHERE conversation_id=?1 AND status IN ('queued','running') ORDER BY trigger_sequence,ordinal", [conversation.id.to_string()])?;
        let current_bout = self
            .query_json(
                "SELECT payload FROM bouts WHERE conversation_id=?1 AND status='open'",
                [conversation.id.to_string()],
            )?
            .into_iter()
            .next();
        Ok(ConversationRecord {
            memory_owner_ids: self.query_json("SELECT DISTINCT json_quote(owner_actor_id) FROM memories WHERE conversation_id=?1 AND status!='forgotten' ORDER BY owner_actor_id", [conversation.id.to_string()])?,
            conversation,
            members,
            latest_message,
            active_run,
            current_bout,
            pending_plans,
        })
    }

    pub(super) fn overview(&self, operator: ActorId) -> Result<SocialOverview> {
        self.operator(operator)?;
        self.overview_for(operator)
    }
    pub(super) fn overview_for(&self, operator: ActorId) -> Result<SocialOverview> {
        let account = self.get("actors", operator)?;
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
            account,
            accounts: self.query_json("SELECT payload FROM actors ORDER BY rowid", [])?,
            address_book: self.address_book(operator)?,
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
        let mut messages: Vec<SocialMessage> = self.query_json("SELECT payload FROM messages WHERE conversation_id=?1 AND status='committed' AND sequence<=?2 ORDER BY sequence DESC LIMIT 200", params![conversation.id.to_string(), trigger.sequence.get() as i64])?;
        if messages.first().map(|m| m.id) != Some(trigger.id) {
            return Err(Error::Forbidden(
                "trigger is outside the visible history".into(),
            ));
        }
        messages.reverse();
        if let Some(id) = trigger.reply_to {
            let quoted: SocialMessage = self.get("messages", id)?;
            if quoted.status == MessageStatus::Committed && !messages.iter().any(|m| m.id == id) {
                messages.push(quoted);
                messages.sort_by_key(|m| m.sequence);
            }
        }
        // Later responders also see committed answers to this same trigger, never future human input.
        let answers = self.query_json("SELECT m.payload FROM messages m JOIN ai_runs r ON r.id=m.run_id JOIN participation_plans p ON p.run_id=r.id WHERE p.trigger_id=?1 AND m.status='committed' ORDER BY m.sequence", [trigger.id.to_string()])?;
        messages.extend(answers);

        let relationship_revision = conversation
            .relationship_id
            .map(|id| {
                self.get::<Relationship>("relationships", id)
                    .map(|r| r.revision)
            })
            .transpose()?;
        Ok(ContextMaterial {
            summaries: self.query_json("SELECT payload FROM conversation_summaries WHERE conversation_id=?1 AND status='ready' AND through_sequence<?2 ORDER BY through_sequence DESC LIMIT 64",params![conversation.id.to_string(),trigger.sequence.get() as i64])?,
            memories: self.memories(operator, scope)?.into_iter().filter(|m| m.status == MaterialStatus::Confirmed).collect(),
            participant_names: self.query_json::<Actor>("SELECT a.payload FROM actors a JOIN memberships m ON m.actor_id=a.id WHERE m.conversation_id=?1 AND m.status='active' ORDER BY a.id", [conversation.id.to_string()])?.into_iter().map(|a|a.display_name).collect(),
            contact: self.contact_record(scope.actor_id)?,
            conversation,
            members: vec![human, ai],
            relationship_revision,
            messages,
            trigger_id: trigger.id,
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
