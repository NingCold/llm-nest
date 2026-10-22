use super::{
    SocialStorageError as Error, SocialStorageResult as Result,
    sqlite::{Db, NewMessage, label, validate_body},
    store::*,
};
use ::social::*;
use connector::SocialChange;
use rusqlite::params;

impl Db<'_> {
    fn response_targets(
        &self,
        c: &Conversation,
        operator: ActorId,
        mentions: &[ActorId],
        reply_to: Option<MessageId>,
    ) -> Result<Vec<ActorId>> {
        let mut targets = if c.kind == ConversationKind::Direct {
            self.query_json::<Membership>("SELECT payload FROM memberships WHERE conversation_id=?1 AND actor_id!=?2 AND status='active'", params![c.id.to_string(), operator.to_string()])?.into_iter().map(|m|m.actor_id).collect()
        } else {
            mentions.to_vec()
        };
        if c.kind == ConversationKind::Group
            && let Some(id) = reply_to
        {
            let reply: SocialMessage = self.get("messages", id)?;
            if reply.sender_id != operator && !targets.contains(&reply.sender_id) {
                targets.push(reply.sender_id);
            }
        }
        Ok(targets)
    }
    pub(super) fn save_plan(&self, p: &ParticipationPlan) -> Result<()> {
        self.execute("INSERT INTO participation_plans VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(id) DO UPDATE SET trigger_id=excluded.trigger_id,trigger_sequence=excluded.trigger_sequence,ordinal=excluded.ordinal,status=excluded.status,run_id=excluded.run_id,payload=excluded.payload", params![p.id.to_string(), p.conversation_id.to_string(), p.actor_id.to_string(), p.trigger_id.map(|id| id.to_string()), p.trigger_sequence.get() as i64, p.ordinal, label(&p.status)?, p.run_id.map(|id| id.to_string()), serde_json::to_string(p)?, p.contact_intent_id.map(|id|id.to_string())])?;
        self.contact_plan_changed(p)?;
        self.event(SocialChange::PlanChanged {
            conversation_id: p.conversation_id,
            plan_id: p.id,
            revision: p.revision,
        })
    }
    pub(super) fn next_plan(&self, id: ConversationId) -> Result<Option<ParticipationPlan>> {
        let head = self.query_json::<ParticipationPlan>("SELECT payload FROM participation_plans WHERE conversation_id=?1 AND status IN ('queued','running') AND (status='running' OR contact_intent_id IS NULL OR julianday(json_extract(payload,'$.not_before'))<=julianday(?2)) ORDER BY (status='running') DESC,trigger_sequence,ordinal,rowid LIMIT 1", params![id.to_string(),self.now.to_rfc3339()])?.into_iter().next();
        Ok(head.filter(|p| p.status == PlanStatus::Queued))
    }
    pub(super) fn send_message(
        &self,
        ctx: &CommandContext,
        input: &SendMessage,
    ) -> Result<SavedMessage> {
        if let Some(id) = self.receipt(ctx, "send_message", input)? {
            let message: SocialMessage = self.get("messages", &id)?;
            self.member(ctx.operator, message.conversation_id)?;
            let c = self.get("conversations", message.conversation_id)?;
            return Ok(SavedMessage {
                response_targets: self.response_targets(
                    &c,
                    ctx.operator,
                    &message.mentions,
                    message.reply_to,
                )?,
                plans: self.query_json(
                    "SELECT payload FROM participation_plans WHERE trigger_id=?1 ORDER BY ordinal",
                    [id],
                )?,
                message,
            });
        }
        validate_body(&input.body)?;
        let sender: Actor = self.get("actors", ctx.operator)?;
        let mut c: Conversation = self.get("conversations", input.conversation_id)?;
        self.member(ctx.operator, c.id)?;
        self.writable(&c)?;
        self.allow_direct_send(&c)?;
        if input.mentions.len() > 63 {
            return Err(Error::Invalid("too many mentions".into()));
        }
        let mut mentions = Vec::new();
        for id in &input.mentions {
            if !mentions.contains(id) {
                mentions.push(*id);
            }
        }
        if let Some(id) = input.reply_to {
            let reply: SocialMessage = self.get("messages", id)?;
            if reply.conversation_id != c.id || reply.status == MessageStatus::Withdrawn {
                return Err(Error::Invalid("invalid reply target".into()));
            }
        }
        let targets = self.response_targets(&c, ctx.operator, &mentions, input.reply_to)?;
        if c.kind == ConversationKind::Direct && mentions.iter().any(|id| !targets.contains(id)) {
            return Err(Error::Invalid("mention is outside this direct chat".into()));
        }
        let mut memberships = Vec::new();
        for id in &targets {
            let actor: Actor = self.get("actors", id)?;
            if actor.kind != ActorKind::Ai {
                return Err(Error::Invalid("response target must be an AI".into()));
            }
            let membership = self.member(*id, c.id)?;
            if actor.status == EntityStatus::Active && !membership.paused {
                memberships.push(membership);
            }
        }
        let preview = input
            .reference
            .as_ref()
            .map(|selection| {
                if c.kind != ConversationKind::Direct || targets.len() != 1 {
                    return Err(Error::Invalid("群话题引用只能发送到唯一私聊".into()));
                }
                self.reference_preview(ctx.operator, targets[0], selection)
            })
            .transpose()?;
        let mut message = self.allocate_message(
            &mut c,
            &sender,
            NewMessage {
                body: input.body.clone(),
                status: MessageStatus::Committed,
                id: MessageId::new(),
                run_id: None,
                reply_to: input.reply_to,
                mentions,
            },
        )?;
        if let (Some(selection), Some(preview)) = (&input.reference, preview) {
            self.attach_reference(&mut message, targets[0], selection.clone(), &preview)?;
        }
        self.human_activity(c.id, !c.paused && !memberships.is_empty())?;
        let mut plans = Vec::new();
        if !c.paused {
            for (ordinal, membership) in memberships.into_iter().enumerate() {
                let p = self.plan_for_message(&c, &message, membership, ordinal as u32)?;
                plans.push(p);
            }
        }
        if targets.is_empty() {
            self.enqueue_automatic(&message)?;
        }
        self.save_receipt(ctx, "send_message", input, message.id)?;
        Ok(SavedMessage {
            message,
            plans,
            response_targets: targets,
        })
    }
    pub(super) fn begin_plan(
        &self,
        operator: ActorId,
        id: PlanId,
        input: BeginRun,
    ) -> Result<AiRun> {
        self.operator(operator)?;
        let mut p: ParticipationPlan = self.get("participation_plans", id)?;
        self.member(operator, p.conversation_id)?;
        if p.status != PlanStatus::Queued
            || self.next_plan(p.conversation_id)?.map(|head| head.id) != Some(id)
            || p.not_before.is_some_and(|t| t > self.now)
            || p.expires_at.is_some_and(|t| t <= self.now)
        {
            return Err(Error::Conflict("plan is no longer the queue head".into()));
        }
        let membership = self.member(p.actor_id, p.conversation_id)?;
        self.expected(membership.revision, p.membership_revision)?;
        if input.snapshot.scope
            != (ContextScope {
                actor_id: p.actor_id,
                conversation_id: p.conversation_id,
            })
            || !input.snapshot.sources.iter().any(
                |s| matches!(s,SourceRef::Message {message_id,..} if Some(*message_id)==p.trigger_id) || matches!(s,SourceRef::ContactIntent {intent_id} if Some(*intent_id)==p.contact_intent_id),
            )
        {
            return Err(Error::Invalid(
                "plan snapshot must include its authorized trigger".into(),
            ));
        }
        for id in &p.input_ids {
            if !input
                .snapshot
                .sources
                .iter()
                .any(|s| matches!(s,SourceRef::Message {message_id,..} if message_id==id))
            {
                return Err(Error::Invalid("合并接话必须包含所有原始消息".into()));
            }
        }
        let run = self.start_run(operator, input, p.trigger_id)?;
        if p.automatic || p.contact_intent_id.is_some() {
            self.reserve_automatic(&p, &run.snapshot)?;
        }
        p.status = PlanStatus::Running;
        p.run_id = Some(run.id);
        p.revision = p.revision.next()?;
        self.save_plan(&p)?;
        Ok(run)
    }
    pub(super) fn finish_plan_for_run(&self, run: &AiRun) -> Result<()> {
        if let Some(mut p) = self
            .query_json::<ParticipationPlan>(
                "SELECT payload FROM participation_plans WHERE run_id=?1",
                [run.id.to_string()],
            )?
            .into_iter()
            .next()
        {
            p.status = match run.status {
                RunStatus::Running => PlanStatus::Running,
                RunStatus::Succeeded => PlanStatus::Succeeded,
                RunStatus::Failed => PlanStatus::Failed,
                RunStatus::Cancelled => PlanStatus::Cancelled,
                RunStatus::Interrupted => PlanStatus::Interrupted,
            };
            p.revision = p.revision.next()?;
            self.save_plan(&p)?;
            self.participation_finished(&p, run)?;
        }
        Ok(())
    }
    pub(super) fn cancel_queued(&self, id: ConversationId, actor: Option<ActorId>) -> Result<()> {
        for mut p in self.query_json::<ParticipationPlan>("SELECT payload FROM participation_plans WHERE conversation_id=?1 AND status='queued' AND (?2 IS NULL OR actor_id=?2)", params![id.to_string(),actor.map(|a|a.to_string())])? {
            p.status=PlanStatus::Cancelled; p.revision=p.revision.next()?; self.save_plan(&p)?;
        }
        Ok(())
    }
    pub(super) fn change_membership(
        &self,
        ctx: &CommandContext,
        input: &ChangeMembership,
    ) -> Result<Conversation> {
        if let Some(id) = self.receipt(ctx, "change_membership", input)? {
            self.member(ctx.operator, input.conversation_id)?;
            return self.get("conversations", id);
        }
        let mut c: Conversation = self.get("conversations", input.conversation_id)?;
        self.member(ctx.operator, c.id)?;
        self.writable(&c)?;
        self.expected(c.revision, input.expected_revision)?;
        if c.kind != ConversationKind::Group {
            return Err(Error::Invalid("membership changes require a group".into()));
        }
        let actor: Actor = self.get("actors", input.actor_id)?;
        if actor.kind != ActorKind::Ai
            || (matches!(input.action, MembershipAction::Add)
                && actor.status != EntityStatus::Active)
        {
            return Err(Error::Invalid(
                "only AI members may be removed; adding requires an active AI".into(),
            ));
        }
        let old = self
            .query_json::<Membership>(
                "SELECT payload FROM memberships WHERE conversation_id=?1 AND actor_id=?2",
                params![c.id.to_string(), actor.id.to_string()],
            )?
            .into_iter()
            .next();
        let status = match input.action {
            MembershipAction::Add => MembershipStatus::Active,
            MembershipAction::Remove => MembershipStatus::Left,
        };
        if old.as_ref().is_some_and(|m| m.status == status)
            || (old.is_none() && status == MembershipStatus::Left)
        {
            return Err(Error::Invalid("member already has this status".into()));
        }
        if status == MembershipStatus::Active {
            let count: i64 = self
                .connection
                .query_row(
                    "SELECT count(*) FROM memberships WHERE conversation_id=?1 AND status='active'",
                    [c.id.to_string()],
                    |r| r.get(0),
                )
                .map_err(|e| super::schema::sql_error(self.path, e))?;
            if count >= 64 {
                return Err(Error::Invalid("group supports at most 64 members".into()));
            }
        }
        let m = Membership {
            conversation_id: c.id,
            actor_id: actor.id,
            status,
            paused: false,
            revision: old
                .as_ref()
                .map(|m| m.revision.next())
                .transpose()?
                .unwrap_or(Revision::ONE),
        };
        self.execute("INSERT INTO memberships(conversation_id,actor_id,status,revision,payload) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(conversation_id,actor_id) DO UPDATE SET status=excluded.status,paused=0,revision=excluded.revision,payload=excluded.payload",params![c.id.to_string(),actor.id.to_string(),label(&m.status)?,m.revision.get() as i64,serde_json::to_string(&m)?])?;
        c.revision = c.revision.next()?;
        c.context_epoch = c.context_epoch.next()?;
        c.participation_epoch = c.participation_epoch.next()?;
        // The roster used by an in-flight request has changed. Keep its persisted draft.
        self.cancel_runs(c.id, RunStatus::Cancelled)?;
        if status == MembershipStatus::Left {
            self.cancel_queued(c.id, Some(actor.id))?;
            self.cancel_contacts(c.id, Some(actor.id), "成员已退出")?;
            self.stale_references_for_member(c.id, actor.id)?;
        }
        self.invalidate_context(c.id)?;
        self.save_conversation(&c)?;
        self.save_receipt(ctx, "change_membership", input, c.id)?;
        Ok(c)
    }
}
