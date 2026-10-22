use super::{SocialStorageError as Error, SocialStorageResult as Result, sqlite::Db, store::*};
use ::social::*;
use connector::SocialChange;
use rusqlite::params;

impl Db<'_> {
    pub(super) fn invalidate_context(&self, id: ConversationId) -> Result<()> {
        for run in
            self.query_json::<AiRun>("SELECT payload FROM ai_runs WHERE status='running'", [])?
        {
            if run
                .snapshot
                .dependencies
                .iter()
                .any(|d| d.conversation_id == id)
            {
                let mut details = self
                    .query_json::<RunDetails>(
                        "SELECT payload FROM run_details WHERE run_id=?1",
                        [run.id.to_string()],
                    )?
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                details.error = Some(RunFailure {
                    code: "context_changed".into(),
                    message: "消息或记忆来源已变化，本次回复已中断".into(),
                });
                self.save_details(run.id, &details)?;
                self.terminate(run, RunStatus::Interrupted, None)?;
            }
        }
        Ok(())
    }
    pub(super) fn cancel_trigger_plans(&self, id: MessageId) -> Result<()> {
        for mut p in self.query_json::<ParticipationPlan>(
            "SELECT payload FROM participation_plans WHERE status='queued' AND (trigger_id=?1 OR EXISTS(SELECT 1 FROM json_each(payload,'$.input_ids') WHERE value=?1))",
            [id.to_string()],
        )? {
            p.status = PlanStatus::Cancelled;
            p.revision = p.revision.next()?;
            self.save_plan(&p)?;
        }
        Ok(())
    }
    pub(super) fn withdraw_message(
        &self,
        ctx: &CommandContext,
        input: &WithdrawMessage,
    ) -> Result<SocialMessage> {
        if let Some(id) = self.receipt(ctx, "withdraw_message", input)? {
            let m: SocialMessage = self.get("messages", id)?;
            self.member(ctx.operator, m.conversation_id)?;
            return Ok(m);
        }
        let mut m: SocialMessage = self.get("messages", input.message_id)?;
        self.member(ctx.operator, m.conversation_id)?;
        let mut c: Conversation = self.get("conversations", m.conversation_id)?;
        self.writable(&c)?;
        if m.sender_id != ctx.operator || m.status != MessageStatus::Committed {
            return Err(Error::Forbidden(
                "only own committed messages can be withdrawn".into(),
            ));
        }
        self.expected(m.revision, input.expected_revision)?;
        m.body = MessageBody::default();
        m.status = MessageStatus::Withdrawn;
        m.revision = m.revision.next()?;
        c.context_epoch = c.context_epoch.next()?;
        c.revision = c.revision.next()?;
        self.update_message(&m, ctx.operator)?;
        self.stale_memories(m.id)?;
        self.stale_summaries(m.id)?;
        self.stale_references_for_message(m.id)?;
        // Old revisions must not revive withdrawn text or attachments.
        self.execute("UPDATE message_revisions SET payload=json_set(payload,'$.body',json('[]'),'$.status','withdrawn') WHERE message_id=?1", [m.id.to_string()])?;
        self.invalidate_context(c.id)?;
        self.cancel_trigger_plans(m.id)?;
        for mut p in self.query_json::<ParticipationPlan>("SELECT p.payload FROM participation_plans p JOIN messages t ON t.id=p.trigger_id WHERE t.reply_to=?1 AND p.status='queued'", [m.id.to_string()])? {
            p.status = PlanStatus::Cancelled;
            p.revision = p.revision.next()?;
            self.save_plan(&p)?;
        }
        self.save_conversation(&c)?;
        self.save_receipt(ctx, "withdraw_message", input, m.id)?;
        Ok(m)
    }
    pub(super) fn pause_member(
        &self,
        ctx: &CommandContext,
        input: &PauseMember,
    ) -> Result<Conversation> {
        if let Some(id) = self.receipt(ctx, "pause_member", input)? {
            self.member(ctx.operator, input.conversation_id)?;
            return self.get("conversations", id);
        }
        let mut c: Conversation = self.get("conversations", input.conversation_id)?;
        self.member(ctx.operator, c.id)?;
        self.writable(&c)?;
        let actor: Actor = self.get("actors", input.actor_id)?;
        if actor.kind != ActorKind::Ai || actor.status != EntityStatus::Active {
            return Err(Error::Invalid(
                "participation control requires an active AI".into(),
            ));
        }
        let mut member = self.member(actor.id, c.id)?;
        self.expected(member.revision, input.expected_revision)?;
        if member.paused && !input.paused {
            self.contact_tick(c.id)?;
        }
        member.paused = input.paused;
        member.revision = member.revision.next()?;
        self.execute("UPDATE memberships SET paused=?3,revision=?4,payload=?5 WHERE conversation_id=?1 AND actor_id=?2", params![c.id.to_string(), actor.id.to_string(),member.paused,member.revision.get() as i64,serde_json::to_string(&member)?])?;
        if member.paused {
            for run in self.query_json::<AiRun>(
                "SELECT payload FROM ai_runs WHERE conversation_id=?1 AND status='running'",
                [c.id.to_string()],
            )? {
                if run.snapshot.scope.actor_id == actor.id {
                    self.terminate(run, RunStatus::Cancelled, None)?;
                }
            }
            self.cancel_queued(c.id, Some(actor.id))?;
        }
        c.revision = c.revision.next()?;
        self.save_conversation(&c)?;
        self.save_receipt(ctx, "pause_member", input, c.id)?;
        Ok(c)
    }
    pub(super) fn close_bout(&self, mut b: Bout) -> Result<Bout> {
        b.status = BoutStatus::Ended;
        b.revision = b.revision.next()?;
        self.cancel_runs(b.conversation_id, RunStatus::Cancelled)?;
        self.cancel_queued(b.conversation_id, None)?;
        self.execute(
            "UPDATE bouts SET status='ended',payload=?2 WHERE id=?1",
            params![b.id.to_string(), serde_json::to_string(&b)?],
        )?;
        self.event(SocialChange::BoutChanged {
            conversation_id: b.conversation_id,
            bout_id: b.id,
            revision: b.revision,
        })?;
        Ok(b)
    }
    pub(super) fn end_conversation(
        &self,
        ctx: &CommandContext,
        input: &EndConversation,
    ) -> Result<Conversation> {
        if let Some(id) = self.receipt(ctx, "end_conversation", input)? {
            self.member(ctx.operator, input.conversation_id)?;
            return self.get("conversations", id);
        }
        let mut c: Conversation = self.get("conversations", input.conversation_id)?;
        self.member(ctx.operator, c.id)?;
        self.writable(&c)?;
        self.expected(c.revision, input.expected_revision)?;
        let bout = self
            .query_json::<Bout>(
                "SELECT payload FROM bouts WHERE conversation_id=?1 AND status='open'",
                [c.id.to_string()],
            )?
            .into_iter()
            .next();
        if let Some(b) = bout {
            self.close_bout(b)?;
        } else {
            self.cancel_runs(c.id, RunStatus::Cancelled)?;
            self.cancel_queued(c.id, None)?;
        }
        c.revision = c.revision.next()?;
        c.participation_epoch = c.participation_epoch.next()?;
        self.save_conversation(&c)?;
        self.save_receipt(ctx, "end_conversation", input, c.id)?;
        let mut state = self.participation(c.id)?.state;
        state.reason = Some(ParticipationReason::UserEnded);
        self.save_participation_state(c.id, &state)?;
        self.get("conversations", c.id)
    }
}
