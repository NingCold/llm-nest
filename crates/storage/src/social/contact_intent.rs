use super::{
    SocialStorageError as Error, SocialStorageResult as Result,
    sqlite::{Db, label},
    store::*,
};
use ::social::*;
use chrono::{DateTime, Duration, Utc};
use rusqlite::params;

impl Db<'_> {
    pub(super) fn contact_settings(&self, id: ConversationId) -> Result<ContactSettings> {
        Ok(self
            .query_json(
                "SELECT payload FROM contact_settings WHERE conversation_id=?1",
                [id.to_string()],
            )?
            .into_iter()
            .next()
            .unwrap_or_default())
    }
    pub(super) fn contact_view(&self, id: ConversationId) -> Result<ContactScheduleView> {
        Ok(ContactScheduleView { settings: self.contact_settings(id)?, intents: self.query_json("SELECT payload FROM contact_intents WHERE conversation_id=?1 AND (status IN ('pending','claimed','executing') OR id IN (SELECT id FROM contact_intents WHERE conversation_id=?1 ORDER BY rowid DESC LIMIT 50)) ORDER BY rowid DESC",[id.to_string()])? })
    }
    fn save_contact(&self, i: &ContactIntent) -> Result<()> {
        self.execute("INSERT INTO contact_intents VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET status=excluded.status,payload=excluded.payload",params![i.id.to_string(),i.conversation_id.to_string(),i.actor_id.to_string(),i.created_by.to_string(),label(&i.status)?,i.due_at.to_rfc3339(),serde_json::to_string(i)?])?;
        self.event(events::SocialChange::ContactScheduleChanged {
            conversation_id: i.conversation_id,
        })
    }
    fn change_contact(
        &self,
        i: &mut ContactIntent,
        status: ContactIntentStatus,
        reason: Option<String>,
    ) -> Result<()> {
        if i.status == status && i.reason == reason {
            return Ok(());
        }
        i.status = status;
        i.reason = reason;
        i.revision = i.revision.next()?;
        i.updated_at = self.now;
        self.save_contact(i)
    }
    fn stop_contact(
        &self,
        mut i: ContactIntent,
        status: ContactIntentStatus,
        reason: &str,
    ) -> Result<()> {
        self.change_contact(&mut i, status, Some(reason.into()))?;
        if let Some(id) = i.plan_id {
            let mut p: ParticipationPlan = self.get("participation_plans", id)?;
            if let Some(run) = p.run_id {
                let run: AiRun = self.get("ai_runs", run)?;
                if run.status == RunStatus::Running {
                    self.terminate(run, RunStatus::Cancelled, None)?;
                }
            } else if p.status == PlanStatus::Queued {
                p.status = PlanStatus::Cancelled;
                p.failure = Some(reason.into());
                p.revision = p.revision.next()?;
                self.save_plan(&p)?;
            }
        }
        Ok(())
    }
    pub(super) fn cancel_blocked_contacts(&self, owner: ActorId, target: ActorId) -> Result<()> {
        for i in self.query_json::<ContactIntent>("SELECT payload FROM contact_intents WHERE status IN ('pending','claimed','executing') AND ((created_by=?1 AND actor_id=?2) OR (created_by=?2 AND actor_id=?1))",params![owner.to_string(),target.to_string()])? {
            self.stop_contact(i, ContactIntentStatus::Cancelled, "联系人已屏蔽")?;
        }
        Ok(())
    }
    pub(super) fn cancel_contacts(
        &self,
        cid: ConversationId,
        actor: Option<ActorId>,
        reason: &str,
    ) -> Result<()> {
        for i in self.query_json("SELECT payload FROM contact_intents WHERE conversation_id=?1 AND (?2 IS NULL OR actor_id=?2) AND status IN ('pending','claimed','executing')",params![cid.to_string(),actor.map(|a|a.to_string())])? {
            self.stop_contact(i,ContactIntentStatus::Cancelled,reason)?;
        }
        Ok(())
    }
    pub(super) fn update_contact_settings(
        &self,
        ctx: &CommandContext,
        input: &UpdateContactSettings,
    ) -> Result<ContactScheduleView> {
        if self.operator(ctx.operator)?.kind != ActorKind::Human {
            return Err(Error::Forbidden("约定联系须由真人明确授权".into()));
        }
        self.member(ctx.operator, input.conversation_id)?;
        if self
            .receipt(ctx, "update_contact_settings", input)?
            .is_some()
        {
            return self.contact_view(input.conversation_id);
        }
        self.writable(&self.get("conversations", input.conversation_id)?)?;
        let old = self.contact_settings(input.conversation_id)?;
        self.expected(old.revision, input.settings.revision)?;
        let mut settings = input.settings.clone();
        if settings
            .quiet_until
            .is_some_and(|t| t <= self.now || t > self.now + Duration::days(365))
        {
            return Err(Error::Invalid(
                "免打扰结束时间须在未来一年内；关闭请清空时间".into(),
            ));
        }
        settings.revision = old.revision.next()?;
        self.execute("INSERT INTO contact_settings VALUES(?1,?2) ON CONFLICT(conversation_id) DO UPDATE SET payload=excluded.payload",params![input.conversation_id.to_string(),serde_json::to_string(&settings)?])?;
        if !settings.enabled {
            self.cancel_contacts(input.conversation_id, None, "主动联系授权已关闭")?;
        } else if settings.quiet_until.is_some_and(|t| t > self.now) {
            // A response already sent to a provider cannot be safely deferred/replayed.
            for i in self.query_json("SELECT payload FROM contact_intents WHERE conversation_id=?1 AND status='executing'",[input.conversation_id.to_string()])? { self.stop_contact(i,ContactIntentStatus::Cancelled,"进入免打扰，已停止本次联系")?; }
        }
        self.event(events::SocialChange::ContactScheduleChanged {
            conversation_id: input.conversation_id,
        })?;
        self.save_receipt(ctx, "update_contact_settings", input, input.conversation_id)?;
        self.contact_view(input.conversation_id)
    }
    pub(super) fn create_contact_intent(
        &self,
        ctx: &CommandContext,
        input: &CreateContactIntent,
    ) -> Result<ContactIntent> {
        if self.operator(ctx.operator)?.kind != ActorKind::Human {
            return Err(Error::Forbidden("约定联系须由真人明确授权".into()));
        }
        self.member(ctx.operator, input.conversation_id)?;
        if let Some(id) = self.receipt(ctx, "create_contact_intent", input)? {
            return self.get("contact_intents", id);
        }
        let c: Conversation = self.get("conversations", input.conversation_id)?;
        self.writable(&c)?;
        if self.blocked_pair(ctx.operator, input.actor_id)? {
            return Err(Error::Forbidden("已屏蔽的联系人不能新建约定联系".into()));
        }
        let a: Actor = self.get("actors", input.actor_id)?;
        let m = self.member(a.id, c.id)?;
        if a.kind != ActorKind::Ai || a.status != EntityStatus::Active || m.paused || c.paused {
            return Err(Error::Invalid("请选择本聊天中可接话的 AI".into()));
        }
        if !self.contact_settings(c.id)?.enabled {
            return Err(Error::Forbidden("请先明确开启本聊天的约定联系授权".into()));
        }
        let due = input.due_at.with_timezone(&Utc);
        let offset = input.due_at.offset().local_minus_utc();
        if input.text.trim().is_empty()
            || input.text.len() > 4000
            || due <= self.now
            || due > self.now + Duration::days(365)
            || input.expires_at <= due
            || input.expires_at > due + Duration::days(7)
            || offset % 60 != 0
            || !(-720 * 60..=840 * 60).contains(&offset)
        {
            return Err(Error::Invalid("约定内容须为 1–4000 字节，时间须在未来一年内，失效时间须晚于约定且不超过 7 天；时区使用分钟级 UTC 偏移".into()));
        }
        let count:i64=self.connection.query_row("SELECT count(*) FROM contact_intents WHERE conversation_id=?1 AND status IN ('pending','claimed','executing')",[c.id.to_string()],|r|r.get(0)).map_err(|e|super::schema::sql_error(self.path,e))?;
        if count >= 64 {
            return Err(Error::Invalid("每个聊天最多保留 64 个待完成约定".into()));
        }
        let i = ContactIntent {
            id: IntentId::new(),
            conversation_id: c.id,
            actor_id: a.id,
            created_by: ctx.operator,
            authorization_id: ctx.request_id,
            text: input.text.trim().into(),
            due_at: due,
            expires_at: input.expires_at,
            utc_offset_minutes: offset / 60,
            status: ContactIntentStatus::Pending,
            plan_id: None,
            revision: Revision::ONE,
            reason: None,
            created_at: self.now,
            updated_at: self.now,
        };
        self.save_contact(&i)?;
        self.save_receipt(ctx, "create_contact_intent", input, i.id)?;
        Ok(i)
    }
    pub(super) fn cancel_contact_intent(
        &self,
        ctx: &CommandContext,
        input: &CancelContactIntent,
    ) -> Result<ContactIntent> {
        self.member(ctx.operator, input.conversation_id)?;
        let i: ContactIntent = self.get("contact_intents", input.intent_id)?;
        if i.conversation_id != input.conversation_id || i.created_by != ctx.operator {
            return Err(Error::Forbidden("只能取消自己在本聊天中的约定".into()));
        }
        if self.receipt(ctx, "cancel_contact_intent", input)?.is_some() {
            return Ok(i);
        }
        self.expected(i.revision, input.expected_revision)?;
        if i.status.terminal() {
            return Err(Error::Conflict("约定已经结束".into()));
        }
        self.stop_contact(i, ContactIntentStatus::Cancelled, "你已取消约定")?;
        self.save_receipt(ctx, "cancel_contact_intent", input, input.intent_id)?;
        self.get("contact_intents", input.intent_id)
    }
    pub(super) fn contact_plan_changed(&self, p: &ParticipationPlan) -> Result<()> {
        let Some(id) = p.contact_intent_id else {
            return Ok(());
        };
        let mut i: ContactIntent = self.get("contact_intents", id)?;
        if i.status.terminal() {
            return Ok(());
        }
        let status = match p.status {
            PlanStatus::Queued => return Ok(()),
            PlanStatus::Running => ContactIntentStatus::Executing,
            PlanStatus::Succeeded => ContactIntentStatus::Delivered,
            PlanStatus::Failed => ContactIntentStatus::Failed,
            PlanStatus::Cancelled => ContactIntentStatus::Cancelled,
            PlanStatus::Interrupted if self.now >= i.expires_at => ContactIntentStatus::Expired,
            PlanStatus::Interrupted => ContactIntentStatus::Uncertain,
        };
        self.change_contact(
            &mut i,
            status,
            p.failure.clone().or_else(|| {
                (status == ContactIntentStatus::Uncertain)
                    .then(|| "生成曾开始但未确认送达，不会自动重试".into())
            }),
        )
    }
    pub(super) fn validate_contact_source(
        &self,
        operator: ActorId,
        scope: ContextScope,
        id: IntentId,
    ) -> Result<ContactIntent> {
        let i: ContactIntent = self.get("contact_intents", id)?;
        let settings = self.contact_settings(scope.conversation_id)?;
        if i.conversation_id != scope.conversation_id
            || i.actor_id != scope.actor_id
            || i.created_by != operator
            || self.blocked_pair(i.created_by, i.actor_id)?
            || !matches!(
                i.status,
                ContactIntentStatus::Claimed | ContactIntentStatus::Executing
            )
            || !settings.enabled
            || settings.quiet_until.is_some_and(|t| t > self.now)
            || i.due_at > self.now
            || i.expires_at <= self.now
        {
            return Err(Error::Conflict("约定联系已取消、过期或正在免打扰".into()));
        }
        Ok(i)
    }
    /// Claim and enqueue atomically. No provider I/O happens inside this transaction.
    pub(super) fn contact_tick(&self, cid: ConversationId) -> Result<Option<DateTime<Utc>>> {
        let settings = self.contact_settings(cid)?;
        let c: Conversation = self.get("conversations", cid)?;
        let mut wake: Option<DateTime<Utc>> = None;
        let intents=self.query_json::<ContactIntent>("SELECT payload FROM contact_intents WHERE conversation_id=?1 AND status IN ('pending','claimed') ORDER BY due_at,rowid",[cid.to_string()])?;
        for mut i in intents {
            if self.blocked_pair(i.created_by, i.actor_id)? {
                self.stop_contact(i, ContactIntentStatus::Cancelled, "联系人已屏蔽")?;
                continue;
            }
            if !settings.enabled {
                self.stop_contact(i, ContactIntentStatus::Cancelled, "主动联系授权已关闭")?;
                continue;
            }
            if i.expires_at <= self.now {
                self.stop_contact(i, ContactIntentStatus::Expired, "已超过约定的有效时间")?;
                continue;
            }
            let mut next = i.due_at;
            if i.due_at <= self.now {
                let actor: Actor = self.get("actors", i.actor_id)?;
                let member=self.query_json::<Membership>("SELECT payload FROM memberships WHERE conversation_id=?1 AND actor_id=?2 AND status='active'",params![cid.to_string(),i.actor_id.to_string()])?.into_iter().next();
                let creator=self.query_json::<Actor>("SELECT a.payload FROM actors a JOIN memberships m ON m.actor_id=a.id WHERE m.conversation_id=?1 AND a.id=?2 AND m.status='active' AND a.status='active'",params![cid.to_string(),i.created_by.to_string()])?;
                if c.status != EntityStatus::Active
                    || actor.status != EntityStatus::Active
                    || member.is_none()
                    || creator.is_empty()
                {
                    self.stop_contact(i, ContactIntentStatus::Cancelled, "账号或成员资格已失效")?;
                    continue;
                }
                let member = member.expect("active member checked");
                if c.paused || member.paused {
                    self.stop_contact(i, ContactIntentStatus::Skipped, "到期时接话已暂停")?;
                    continue;
                }
                if let Some(until) = settings.quiet_until.filter(|t| *t > self.now) {
                    if settings.quiet_action == QuietAction::Skip {
                        self.stop_contact(
                            i,
                            ContactIntentStatus::Skipped,
                            "到期时处于免打扰，已按设置跳过",
                        )?;
                        continue;
                    }
                    next = until.min(i.expires_at);
                    let status = if i.plan_id.is_some() {
                        ContactIntentStatus::Claimed
                    } else {
                        ContactIntentStatus::Pending
                    };
                    self.change_contact(
                        &mut i,
                        status,
                        Some("免打扰结束后再联系，超过失效时间则跳过".into()),
                    )?;
                    if let Some(id) = i.plan_id {
                        let mut p: ParticipationPlan = self.get("participation_plans", id)?;
                        if p.not_before != Some(next) {
                            p.not_before = Some(next);
                            p.revision = p.revision.next()?;
                            self.save_plan(&p)?;
                        }
                    }
                } else if let Some(id) = i.plan_id {
                    let mut p: ParticipationPlan = self.get("participation_plans", id)?;
                    if p.not_before.is_some_and(|t| t > self.now) {
                        p.not_before = Some(self.now);
                        p.revision = p.revision.next()?;
                        self.save_plan(&p)?;
                    }
                    continue;
                } else {
                    let id = PlanId::new();
                    i.plan_id = Some(id);
                    self.change_contact(&mut i, ContactIntentStatus::Claimed, None)?;
                    let ordinal:u32=self.connection.query_row("SELECT coalesce(max(ordinal)+1,0) FROM participation_plans WHERE conversation_id=?1 AND trigger_sequence=?2",params![cid.to_string(),c.last_sequence.get() as i64],|r|r.get(0)).map_err(|e|super::schema::sql_error(self.path,e))?;
                    self.save_plan(&ParticipationPlan {
                        id,
                        conversation_id: cid,
                        actor_id: i.actor_id,
                        trigger_id: None,
                        contact_intent_id: Some(i.id),
                        trigger_sequence: c.last_sequence,
                        ordinal,
                        membership_revision: member.revision,
                        status: PlanStatus::Queued,
                        run_id: None,
                        failure: None,
                        revision: Revision::ONE,
                        created_at: self.now,
                        automatic: false,
                        input_ids: vec![],
                        not_before: Some(self.now),
                        expires_at: Some(i.expires_at),
                        policy_revision: None,
                    })?;
                    continue;
                }
            }
            wake = Some(wake.map_or(next, |w| w.min(next)));
        }
        Ok(wake)
    }
}
