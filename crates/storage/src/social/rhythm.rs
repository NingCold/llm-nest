use super::{SocialStorageError as Error, SocialStorageResult as Result, sqlite::Db, store::*};
use ::social::*;
use chrono::Duration;
use rusqlite::params;

impl Db<'_> {
    pub(super) fn participation(&self, id: ConversationId) -> Result<ParticipationView> {
        let policy = self
            .query_json(
                "SELECT payload FROM participation_policies WHERE conversation_id=?1",
                [id.to_string()],
            )?
            .into_iter()
            .next()
            .unwrap_or_default();
        let state = self
            .query_json(
                "SELECT payload FROM participation_state WHERE conversation_id=?1",
                [id.to_string()],
            )?
            .into_iter()
            .next()
            .unwrap_or_default();
        // A backwards wall-clock adjustment cannot reopen a previously spent day.
        let today = self.now.format("%Y-%m-%d").to_string();
        let day: String = self.connection.query_row("SELECT max(?2,coalesce(max(day),?2)) FROM participation_daily_budget WHERE conversation_id=?1", params![id.to_string(), today], |r|r.get(0)).map_err(|e|super::schema::sql_error(self.path,e))?;
        let (turns, tokens): (u32,u32) = self.connection.query_row("SELECT coalesce(sum(turns),0),coalesce(sum(tokens),0) FROM participation_daily_budget WHERE conversation_id=?1 AND day=?2",params![id.to_string(),day],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|super::schema::sql_error(self.path,e))?;
        Ok(ParticipationView {
            policy,
            state,
            budget_day: day,
            auto_turns_today: turns,
            reserved_tokens_today: tokens,
        })
    }
    pub(super) fn save_participation_state(
        &self,
        id: ConversationId,
        state: &ParticipationState,
    ) -> Result<()> {
        self.execute("INSERT INTO participation_state VALUES(?1,?2) ON CONFLICT(conversation_id) DO UPDATE SET payload=excluded.payload",params![id.to_string(),serde_json::to_string(state)?])?;
        self.event(events::SocialChange::ParticipationChanged {
            conversation_id: id,
        })
    }
    pub(super) fn update_participation(
        &self,
        ctx: &CommandContext,
        input: &UpdateParticipation,
    ) -> Result<Conversation> {
        if let Some(id) = self.receipt(ctx, "update_participation", input)? {
            self.member(ctx.operator, input.conversation_id)?;
            return self.get("conversations", id);
        }
        let c: Conversation = self.get("conversations", input.conversation_id)?;
        self.member(ctx.operator, c.id)?;
        self.writable(&c)?;
        let mut view = self.participation(c.id)?;
        self.expected(view.policy.revision, input.policy.revision)?;
        let p = &input.policy;
        if p.merge_delay_ms > 2000
            || !(30..=3600).contains(&p.idle_seconds)
            || !(1..=6).contains(&p.max_consecutive)
            || !(1..=48).contains(&p.max_auto_per_bout)
            || !(1..=200).contains(&p.max_auto_per_day)
            || !(1024..=4_000_000).contains(&p.daily_token_budget)
            || (c.kind != ConversationKind::Group && p.limited_auto)
        {
            return Err(Error::Invalid(
                "接话规则超出允许范围，自动接话仅适用于群聊".into(),
            ));
        }
        let mut policy = p.clone();
        policy.revision = policy.revision.next()?;
        self.execute("INSERT INTO participation_policies VALUES(?1,?2) ON CONFLICT(conversation_id) DO UPDATE SET payload=excluded.payload",params![c.id.to_string(),serde_json::to_string(&policy)?])?;
        // Consent changes cancel automatic work, including work already in flight.
        // Explicit queued replies keep their original timing and authorization.
        self.cancel_automatic(c.id)?;
        view.state.reason = Some(ParticipationReason::PolicyChanged);
        self.save_participation_state(c.id, &view.state)?;
        self.save_receipt(ctx, "update_participation", input, c.id)?;
        self.get("conversations", c.id)
    }
    fn cancel_automatic(&self, id: ConversationId) -> Result<()> {
        for mut p in self.query_json::<ParticipationPlan>("SELECT payload FROM participation_plans WHERE conversation_id=?1 AND status IN ('queued','running') AND json_extract(payload,'$.automatic')=1",[id.to_string()])? {
            if let Some(run_id) = p.run_id {
                self.terminate(self.get("ai_runs",run_id)?,RunStatus::Cancelled,None)?;
            } else {
                p.status = PlanStatus::Cancelled;
                p.revision = p.revision.next()?;
                self.save_plan(&p)?;
            }
        }
        Ok(())
    }
    pub(super) fn human_activity(&self, id: ConversationId, explicit_reply: bool) -> Result<()> {
        // A new human turn supersedes only unstarted automatic replies.
        for mut p in self.query_json::<ParticipationPlan>("SELECT payload FROM participation_plans WHERE conversation_id=?1 AND status='queued' AND json_extract(payload,'$.automatic')=1",[id.to_string()])? {
            p.status = PlanStatus::Cancelled;
            p.revision = p.revision.next()?;
            self.save_plan(&p)?;
        }
        let mut state = self.participation(id)?.state;
        state.consecutive_ai = 0;
        let open: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM bouts WHERE conversation_id=?1 AND status='open'",
                [id.to_string()],
                |r| r.get(0),
            )
            .map_err(|e| super::schema::sql_error(self.path, e))?;
        if open == 0 {
            state.bout_auto_turns = 0;
        }
        state.last_activity = Some(state.last_activity.map_or(self.now, |t| t.max(self.now)));
        state.reason = Some(if explicit_reply {
            ParticipationReason::InputPending
        } else {
            ParticipationReason::Waiting
        });
        self.save_participation_state(id, &state)
    }
    pub(super) fn start_participation_bout(&self, id: ConversationId) -> Result<()> {
        let mut state = self.participation(id)?.state;
        state.bout_auto_turns = 0;
        state.consecutive_ai = 0;
        // The caller owns the current Conversation revision while creating a run.
        self.execute("INSERT INTO participation_state VALUES(?1,?2) ON CONFLICT(conversation_id) DO UPDATE SET payload=excluded.payload",params![id.to_string(),serde_json::to_string(&state)?])?;
        Ok(())
    }
    pub(super) fn plan_for_message(
        &self,
        c: &Conversation,
        message: &SocialMessage,
        membership: Membership,
        ordinal: u32,
    ) -> Result<ParticipationPlan> {
        let policy = self.participation(c.id)?.policy;
        let delay = if c.kind == ConversationKind::Direct {
            policy.merge_delay_ms
        } else {
            0
        };
        let plain = |m: &SocialMessage| {
            m.reply_to.is_none() && m.context_reference.is_none() && m.mentions.is_empty()
        };
        let previous = if delay > 0 && plain(message) {
            self.query_json::<ParticipationPlan>("SELECT payload FROM participation_plans WHERE conversation_id=?1 AND actor_id=?2 AND status='queued' AND contact_intent_id IS NULL ORDER BY trigger_sequence DESC LIMIT 1",params![c.id.to_string(),membership.actor_id.to_string()])?.into_iter().next()
        } else {
            None
        };
        let mut p = ParticipationPlan {
            automatic: false,
            input_ids: vec![message.id],
            not_before: Some(self.now + Duration::milliseconds(delay.into())),
            expires_at: Some(self.now + Duration::minutes(5)),
            policy_revision: Some(policy.revision),
            id: PlanId::new(),
            conversation_id: c.id,
            actor_id: membership.actor_id,
            trigger_id: Some(message.id),
            contact_intent_id: None,
            trigger_sequence: message.sequence,
            ordinal,
            membership_revision: membership.revision,
            status: PlanStatus::Queued,
            run_id: None,
            failure: None,
            revision: Revision::ONE,
            created_at: self.now,
        };
        if let Some(old) = previous {
            let old_message: SocialMessage =
                self.get("messages", old.trigger_id.expect("message plan"))?;
            if !old.automatic
                && old.membership_revision == membership.revision
                && old.input_ids.len() < 20
                && old.not_before.is_some_and(|t| t > self.now)
                && plain(&old_message)
                && old_message.status == MessageStatus::Committed
            {
                p.id = old.id;
                p.created_at = old.created_at;
                p.revision = old.revision.next()?;
                p.input_ids = old.input_ids;
                if p.input_ids.is_empty() {
                    p.input_ids.push(old.trigger_id.expect("message plan"));
                }
                p.input_ids.push(message.id);
            }
        }
        self.save_plan(&p)?;
        Ok(p)
    }
    fn auto_limit(view: &ParticipationView, reservation: u32) -> Option<ParticipationReason> {
        if view.state.consecutive_ai >= view.policy.max_consecutive
            || view.state.bout_auto_turns >= view.policy.max_auto_per_bout
        {
            Some(ParticipationReason::TurnLimit)
        } else if view.auto_turns_today >= view.policy.max_auto_per_day {
            Some(ParticipationReason::DailyLimit)
        } else if view.reserved_tokens_today.saturating_add(reservation)
            > view.policy.daily_token_budget
        {
            Some(ParticipationReason::TokenLimit)
        } else {
            None
        }
    }
    pub(super) fn enqueue_automatic(&self, message: &SocialMessage) -> Result<()> {
        let c: Conversation = self.get("conversations", message.conversation_id)?;
        let mut view = self.participation(c.id)?;
        if c.kind != ConversationKind::Group
            || c.paused
            || c.status != EntityStatus::Active
            || !view.policy.limited_auto
        {
            return Ok(());
        }
        let pending:i64 = self.connection.query_row("SELECT count(*) FROM participation_plans WHERE conversation_id=?1 AND status IN ('queued','running')",[c.id.to_string()],|r|r.get(0)).map_err(|e|super::schema::sql_error(self.path,e))?;
        let sender: Actor = self.get("actors", message.sender_id)?;
        if pending > 0 && sender.kind != ActorKind::Human {
            return Ok(());
        }
        let candidates = self.query_json::<Membership>("SELECT m.payload FROM memberships m JOIN actors a ON a.id=m.actor_id WHERE m.conversation_id=?1 AND m.status='active' AND m.paused=0 AND a.status='active' AND a.kind='ai' AND a.id!=?2 ORDER BY m.actor_id",params![c.id.to_string(),message.sender_id.to_string()])?;
        let candidate = candidates
            .iter()
            .find(|m| m.actor_id.to_string() > message.sender_id.to_string())
            .or(candidates.first());
        view.state.reason = Self::auto_limit(&view, 0).or_else(|| {
            candidate
                .is_none()
                .then_some(ParticipationReason::NoCandidate)
        });
        if view.state.reason.is_none() {
            let member = candidate.expect("candidate checked");
            self.save_plan(&ParticipationPlan {
                automatic: true,
                input_ids: vec![message.id],
                not_before: Some(self.now + Duration::milliseconds(800)),
                expires_at: Some(self.now + Duration::minutes(5)),
                policy_revision: Some(view.policy.revision),
                id: PlanId::new(),
                conversation_id: c.id,
                actor_id: member.actor_id,
                trigger_id: Some(message.id),
                contact_intent_id: None,
                trigger_sequence: message.sequence,
                ordinal: 0,
                membership_revision: member.revision,
                status: PlanStatus::Queued,
                run_id: None,
                failure: None,
                revision: Revision::ONE,
                created_at: self.now,
            })?;
        }
        self.save_participation_state(c.id, &view.state)
    }
    pub(super) fn reserve_automatic(
        &self,
        p: &ParticipationPlan,
        snapshot: &ContextSnapshot,
    ) -> Result<()> {
        let mut view = self.participation(p.conversation_id)?;
        if p.automatic
            && (!view.policy.limited_auto || p.policy_revision != Some(view.policy.revision))
        {
            return Err(Error::Conflict("自动接话授权已变化".into()));
        }
        // Reserve the entire allowed input plus output capacity, not unverified provider usage.
        let tokens = snapshot
            .budget
            .input_token_limit
            .get()
            .saturating_add(snapshot.budget.reserved_output_tokens.get())
            .min(u32::MAX as u64) as u32;
        if let Some(reason) = Self::auto_limit(&view, tokens) {
            return Err(Error::ParticipationLimit {
                reason,
                message: match reason {
                    ParticipationReason::TokenLimit => "主动调用的每日 token 预留额度不足",
                    ParticipationReason::DailyLimit => "主动调用已达到每日次数上限",
                    _ => "主动调用已达到本轮次数上限",
                },
            });
        }
        self.execute("INSERT INTO participation_daily_budget VALUES(?1,?2,1,?3) ON CONFLICT(conversation_id,day) DO UPDATE SET turns=turns+1,tokens=tokens+excluded.tokens",params![p.conversation_id.to_string(),view.budget_day,tokens])?;
        view.state.bout_auto_turns += 1;
        self.save_participation_state(p.conversation_id, &view.state)
    }
    pub(super) fn participation_finished(&self, p: &ParticipationPlan, run: &AiRun) -> Result<()> {
        let mut state = self.participation(p.conversation_id)?.state;
        state.last_activity = Some(state.last_activity.map_or(self.now, |t| t.max(self.now)));
        state.reason = Some(ParticipationReason::Waiting);
        if run.status == RunStatus::Succeeded {
            state.consecutive_ai += 1;
        }
        self.save_participation_state(p.conversation_id, &state)?;
        if run.status == RunStatus::Succeeded && p.contact_intent_id.is_none() {
            let newer_human:i64 = self.connection.query_row("SELECT count(*) FROM messages m JOIN actors a ON a.id=m.sender_id WHERE m.conversation_id=?1 AND m.sequence>?2 AND m.status='committed' AND a.kind='human'",params![p.conversation_id.to_string(),p.trigger_sequence.get() as i64],|r|r.get(0)).map_err(|e|super::schema::sql_error(self.path,e))?;
            if newer_human == 0 {
                self.enqueue_automatic(&self.get("messages", run.message_id)?)?;
            }
        }
        Ok(())
    }
    pub(super) fn participation_tick(&self, id: ConversationId) -> Result<ParticipationTick> {
        // After downtime, close an idle old bout before a newly due contact is claimed.
        let old = self.participation(id)?;
        if old
            .state
            .last_activity
            .is_some_and(|t| t + Duration::seconds(old.policy.idle_seconds.into()) <= self.now)
        {
            let idle:Option<Bout>=self.query_json("SELECT payload FROM bouts WHERE conversation_id=?1 AND status='open' AND NOT EXISTS(SELECT 1 FROM participation_plans WHERE conversation_id=?1 AND status IN ('queued','running')) AND NOT EXISTS(SELECT 1 FROM ai_runs WHERE conversation_id=?1 AND status='running')",[id.to_string()])?.into_iter().next();
            if let Some(b) = idle {
                self.close_bout(b)?;
                let mut state = old.state;
                state.reason = Some(ParticipationReason::Idle);
                self.save_participation_state(id, &state)?;
            }
        }
        let contact_wake = self.contact_tick(id)?;
        let mut view = self.participation(id)?;
        while let Some(mut p) = self.next_plan(id)? {
            if p.expires_at.is_some_and(|t| t <= self.now) {
                p.status = PlanStatus::Cancelled;
                p.failure = Some("接话计划已过期，等待新消息".into());
                p.revision = p.revision.next()?;
                self.save_plan(&p)?;
                view.state.reason = Some(ParticipationReason::Expired);
                self.save_participation_state(id, &view.state)?;
                continue;
            }
            let wake_at = p.not_before.filter(|t| *t > self.now);
            return Ok(ParticipationTick {
                plan: if wake_at.is_none() { Some(p) } else { None },
                wake_at: earliest(wake_at, contact_wake),
            });
        }
        let bout: Option<Bout> = self
            .query_json(
                "SELECT payload FROM bouts WHERE conversation_id=?1 AND status='open'",
                [id.to_string()],
            )?
            .into_iter()
            .next();
        let running: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM ai_runs WHERE conversation_id=?1 AND status='running'",
                [id.to_string()],
                |r| r.get(0),
            )
            .map_err(|e| super::schema::sql_error(self.path, e))?;
        let mut wake_at = None;
        if let Some(bout) = bout
            && running == 0
        {
            let activity = match view.state.last_activity {
                Some(t) => t,
                None => {
                    view.state.last_activity = Some(self.now);
                    self.save_participation_state(id, &view.state)?;
                    self.now
                }
            };
            let deadline = activity + Duration::seconds(view.policy.idle_seconds.into());
            if deadline <= self.now {
                self.close_bout(bout)?;
                view.state.reason = Some(ParticipationReason::Idle);
                self.save_participation_state(id, &view.state)?;
            } else {
                wake_at = Some(deadline);
            }
        }
        Ok(ParticipationTick {
            plan: None,
            wake_at: earliest(wake_at, contact_wake),
        })
    }
}

fn earliest(
    a: Option<chrono::DateTime<chrono::Utc>>,
    b: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}
