use super::{
    SocialStorageError as Error, SocialStorageResult as Result,
    sqlite::{Db, label},
    store::*,
};
use ::social::*;
use connector::SocialChange;
use rusqlite::params;

impl Db<'_> {
    fn summary_settings(&self, id: ConversationId) -> Result<SummarySettings> {
        Ok(self
            .optional("summary_settings", id)?
            .unwrap_or(SummarySettings {
                id,
                enabled: false,
                binding: None,
                revision: Revision::ZERO,
            }))
    }

    pub(super) fn summaries(
        &self,
        operator: ActorId,
        id: ConversationId,
        before: Option<Sequence>,
    ) -> Result<SummaryState> {
        self.operator(operator)?;
        self.member(operator, id)?;
        let mut segments = self.query_json("SELECT payload FROM conversation_summaries WHERE conversation_id=?1 AND (?2 IS NULL OR through_sequence<?2) ORDER BY through_sequence DESC LIMIT 51", params![id.to_string(), before.map(|s|s.get() as i64)])?;
        let has_more = segments.len() > 50;
        segments.truncate(50);
        Ok(SummaryState {
            settings: self.summary_settings(id)?,
            segments,
            has_more,
        })
    }

    pub(super) fn configure_summaries(
        &self,
        ctx: &CommandContext,
        id: ConversationId,
        input: &ConfigureSummaries,
    ) -> Result<SummarySettings> {
        let receipt_input = (id, input);
        let receipt = self.receipt(ctx, "configure_summaries", &receipt_input)?;
        self.member(ctx.operator, id)?;
        let c: Conversation = self.get("conversations", id)?;
        self.writable(&c)?;
        let mut settings = self.summary_settings(id)?;
        if receipt.is_some() {
            return Ok(settings);
        }
        self.expected(settings.revision, input.expected_revision)?;
        if input.enabled && input.binding.is_none() {
            return Err(Error::Invalid("开启摘要前请选择专用模型".into()));
        }
        if let Some(binding) = &input.binding
            && (binding.provider_id.trim().is_empty()
                || binding.model_id.trim().is_empty()
                || binding.max_tokens == 0
                || binding.max_tokens > 16384)
        {
            return Err(Error::Invalid(
                "摘要模型及输出预算无效（最多 16384 tokens）".into(),
            ));
        }
        settings.enabled = input.enabled;
        settings.binding = input.binding.clone();
        settings.revision = settings.revision.next()?;
        self.execute("INSERT INTO summary_settings VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", params![id.to_string(),serde_json::to_string(&settings)?])?;
        self.interrupt_summaries(id, "摘要设置已修改，请手动重试")?;
        self.event(SocialChange::SummariesChanged {
            conversation_id: id,
        })?;
        self.save_receipt(ctx, "configure_summaries", &receipt_input, id)?;
        Ok(settings)
    }

    pub(super) fn save_summary(&self, summary: &ConversationSummary) -> Result<()> {
        let payload = serde_json::to_string(summary)?;
        self.execute("INSERT INTO conversation_summaries VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET status=excluded.status,revision=excluded.revision,payload=excluded.payload", params![summary.id.to_string(), summary.conversation_id.to_string(), summary.from_sequence.get() as i64, summary.through_sequence.get() as i64, label(&summary.status)?, summary.revision.get() as i64, payload])?;
        self.execute(
            "INSERT INTO summary_revisions VALUES(?1,?2,?3)",
            params![
                summary.id.to_string(),
                summary.revision.get() as i64,
                payload
            ],
        )?;
        self.execute(
            "DELETE FROM summary_message_sources WHERE summary_id=?1",
            [summary.id.to_string()],
        )?;
        for source in &summary.sources {
            if let SourceRef::Message {
                message_id,
                revision,
                ..
            } = source
            {
                self.execute(
                    "INSERT INTO summary_message_sources VALUES(?1,?2,?3)",
                    params![
                        summary.id.to_string(),
                        message_id.to_string(),
                        revision.get() as i64
                    ],
                )?;
            }
        }
        self.event(SocialChange::SummariesChanged {
            conversation_id: summary.conversation_id,
        })?;
        Ok(())
    }

    pub(super) fn queue_summary(
        &self,
        ctx: &CommandContext,
        input: &QueueSummary,
    ) -> Result<Option<ConversationSummary>> {
        // Budget and automatic mode are server choices; request identity covers the user's intent.
        let intent = (input.conversation_id, input.retry, input.automatic);
        let receipt = self.receipt(ctx, "queue_summary", &intent)?;
        let member = self.member(ctx.operator, input.conversation_id)?;
        if let Some(id) = receipt {
            return self.optional("conversation_summaries", id);
        }
        let c: Conversation = self.get("conversations", input.conversation_id)?;
        self.writable(&c)?;
        let settings = self.summary_settings(c.id)?;
        self.expected(settings.revision, input.settings_revision)?;
        if !settings.enabled {
            return Err(Error::Invalid("请先开启窗口摘要".into()));
        }
        let binding = settings
            .binding
            .ok_or_else(|| Error::Invalid("摘要缺少模型".into()))?;
        if !self.query_json::<ConversationSummary>("SELECT payload FROM conversation_summaries WHERE conversation_id=?1 AND status IN ('queued','running')", [c.id.to_string()])?.is_empty() {
            return Err(Error::Conflict("本窗口已有摘要任务".into()));
        }
        let old = if let Some((id, revision)) = input.retry {
            let old: ConversationSummary = self.get("conversation_summaries", id)?;
            if old.conversation_id != c.id {
                return Err(Error::Forbidden("摘要不属于本窗口".into()));
            }
            self.expected(old.revision, revision)?;
            if old.status == SummaryStatus::Ready {
                return Err(Error::Invalid("有效摘要无需重试".into()));
            }
            Some(old)
        } else {
            None
        };
        let mut candidates: Vec<SocialMessage> = if let Some(old) = &old {
            self.query_json("SELECT payload FROM messages WHERE conversation_id=?1 AND status='committed' AND sequence>=?2 AND sequence<=?3 ORDER BY sequence", params![c.id.to_string(),old.from_sequence.get() as i64,old.through_sequence.get() as i64])?
        } else {
            // Keep the newest twelve committed messages as originals; segments never overlap.
            self.query_json("SELECT payload FROM messages WHERE conversation_id=?1 AND status='committed' AND sequence>COALESCE((SELECT MAX(through_sequence) FROM conversation_summaries WHERE conversation_id=?1),0) AND sequence<(SELECT sequence FROM messages WHERE conversation_id=?1 AND status='committed' ORDER BY sequence DESC LIMIT 1 OFFSET 11) AND sequence<COALESCE((SELECT MIN(sequence) FROM messages WHERE conversation_id=?1 AND status='generating'),9223372036854775807) ORDER BY sequence LIMIT 32", [c.id.to_string()])?
        };
        if candidates.is_empty() {
            if old.is_some() {
                return Err(Error::Invalid("该分段的原文均已撤回，无需重建".into()));
            }
            if !input.automatic {
                self.save_receipt(ctx, "queue_summary", &intent, "none")?;
            }
            return Ok(None);
        }
        // A conservative bound on the JSON input; the feature checks the complete prompt again.
        let costs: Vec<usize> = candidates
            .iter()
            .map(|m| {
                serde_json::to_string(&m.body).map(|body| body.len() + m.sender_name.len() + 256)
            })
            .collect::<std::result::Result<_, _>>()?;
        if old.is_none()
            && input.automatic
            && candidates.len() < 16
            && costs.iter().sum::<usize>() < input.input_budget as usize
        {
            return Ok(None);
        }
        let mut used = 0;
        let count = costs
            .iter()
            .take_while(|cost| {
                used += **cost;
                used <= input.input_budget as usize
            })
            .count();
        if old.is_none() {
            candidates.truncate(count.max(1));
        }
        let oversized = count == 0 || (old.is_some() && count < candidates.len());
        let summary = ConversationSummary {
            id: old.as_ref().map_or_else(SummaryId::new, |s| s.id),
            conversation_id: c.id,
            from_sequence: old
                .as_ref()
                .map_or(candidates[0].sequence, |s| s.from_sequence),
            through_sequence: old
                .as_ref()
                .map_or(candidates.last().expect("nonempty").sequence, |s| {
                    s.through_sequence
                }),
            text: String::new(),
            status: if oversized {
                SummaryStatus::Failed
            } else {
                SummaryStatus::Queued
            },
            revision: old
                .as_ref()
                .map(|s| s.revision.next())
                .transpose()?
                .unwrap_or(Revision::ONE),
            sources: candidates
                .iter()
                .map(|m| SourceRef::Message {
                    conversation_id: c.id,
                    message_id: m.id,
                    revision: m.revision,
                })
                .collect(),
            binding,
            settings_revision: settings.revision,
            requested_by: ctx.operator,
            membership_revision: member.revision,
            participation_epoch: c.participation_epoch,
            attempt: old
                .as_ref()
                .map(|s| s.attempt.next())
                .transpose()?
                .unwrap_or(Counter::ONE),
            error: oversized.then(|| "原文超出摘要输入预算，请选择更大上下文的模型后重试".into()),
            created_at: old.as_ref().map_or(self.now, |s| s.created_at),
            updated_at: self.now,
        };
        self.save_summary(&summary)?;
        self.save_receipt(ctx, "queue_summary", &intent, summary.id)?;
        Ok(Some(summary))
    }

    fn summary_messages(&self, summary: &ConversationSummary) -> Result<Vec<SocialMessage>> {
        summary
            .sources
            .iter()
            .map(|source| {
                let SourceRef::Message {
                    conversation_id,
                    message_id,
                    revision,
                } = source
                else {
                    return Err(Error::Invalid("摘要只能依赖原消息".into()));
                };
                let m: SocialMessage = self.get("messages", message_id)?;
                if *conversation_id != summary.conversation_id
                    || m.conversation_id != *conversation_id
                    || m.revision != *revision
                    || m.status != MessageStatus::Committed
                {
                    return Err(Error::Conflict("摘要来源已变化".into()));
                }
                Ok(m)
            })
            .collect()
    }
    fn summary_task_current(&self, s: &ConversationSummary) -> Result<()> {
        self.operator(s.requested_by)?;
        let member = self.member(s.requested_by, s.conversation_id)?;
        let c: Conversation = self.get("conversations", s.conversation_id)?;
        self.writable(&c)?;
        let settings = self.summary_settings(c.id)?;
        if !settings.enabled
            || settings.revision != s.settings_revision
            || member.revision != s.membership_revision
            || c.participation_epoch != s.participation_epoch
        {
            return Err(Error::Conflict("摘要设置或窗口参与状态已变化".into()));
        }
        Ok(())
    }
    pub(super) fn begin_summary(
        &self,
        operator: ActorId,
        id: SummaryId,
    ) -> Result<Option<SummaryWork>> {
        self.operator(operator)?;
        let mut summary: ConversationSummary = self.get("conversation_summaries", id)?;
        self.member(operator, summary.conversation_id)?;
        if operator != summary.requested_by {
            return Err(Error::Forbidden("摘要任务操作者不一致".into()));
        }
        if summary.status != SummaryStatus::Queued {
            return Ok(None);
        }
        self.summary_task_current(&summary)?;
        let messages = self.summary_messages(&summary)?;
        summary.status = SummaryStatus::Running;
        summary.revision = summary.revision.next()?;
        summary.updated_at = self.now;
        self.save_summary(&summary)?;
        Ok(Some(SummaryWork { summary, messages }))
    }
    pub(super) fn finish_summary(
        &self,
        operator: ActorId,
        input: FinishSummary,
    ) -> Result<ConversationSummary> {
        self.operator(operator)?;
        let mut s: ConversationSummary = self.get("conversation_summaries", input.id)?;
        if operator != s.requested_by {
            return Err(Error::Forbidden("摘要任务操作者不一致".into()));
        }
        // Invalidation wins over every late completion, including a failed provider stream.
        if !matches!(s.status, SummaryStatus::Queued | SummaryStatus::Running)
            || s.revision != input.expected_revision
        {
            return Ok(s);
        }
        if !matches!(
            input.status,
            SummaryStatus::Ready | SummaryStatus::Failed | SummaryStatus::Interrupted
        ) {
            return Err(Error::Invalid("无效摘要终态".into()));
        }
        let valid = self
            .summary_task_current(&s)
            .and_then(|_| self.summary_messages(&s).map(|_| ()));
        if let Err(error) = valid {
            match error {
                Error::Conflict(_) | Error::Forbidden(_) | Error::Invalid(_) => {
                    s.status = SummaryStatus::Stale;
                    s.error = Some("来源或窗口状态已变化，旧结果已丢弃".into());
                }
                error => return Err(error),
            }
        } else {
            if input.status == SummaryStatus::Ready
                && (input.text.trim().is_empty() || input.text.len() > 16384)
            {
                return Err(Error::Invalid("摘要正文应为 1–16384 字节".into()));
            }
            s.status = input.status;
            s.error = input.error;
            if s.status == SummaryStatus::Ready {
                s.text = input.text;
            }
        }
        s.revision = s.revision.next()?;
        s.updated_at = self.now;
        self.save_summary(&s)?;
        Ok(s)
    }
    pub(super) fn validate_summary_source(
        &self,
        scope: ContextScope,
        id: SummaryId,
        revision: Revision,
    ) -> Result<()> {
        let s: ConversationSummary = self.get("conversation_summaries", id)?;
        if s.conversation_id != scope.conversation_id {
            return Err(Error::Forbidden("摘要不属于本窗口".into()));
        }
        if s.status != SummaryStatus::Ready || s.revision != revision {
            return Err(Error::Conflict("摘要已失效".into()));
        }
        self.summary_messages(&s)?;
        Ok(())
    }
    pub(super) fn stale_summaries(&self, message: MessageId) -> Result<()> {
        for mut s in self.query_json::<ConversationSummary>("SELECT s.payload FROM conversation_summaries s JOIN summary_message_sources m ON m.summary_id=s.id WHERE m.message_id=?1 AND s.status!='stale'",[message.to_string()])? {
            s.status=SummaryStatus::Stale; s.text.clear(); s.error=Some("来源已修改或撤回，请核对后重建".into()); s.revision=s.revision.next()?; s.updated_at=self.now;
            self.save_summary(&s)?;
            self.execute("UPDATE summary_revisions SET payload=json_set(payload,'$.text','') WHERE summary_id=?1",[s.id.to_string()])?;
        }
        Ok(())
    }
    pub(super) fn interrupt_summaries(&self, id: ConversationId, reason: &str) -> Result<()> {
        for mut s in self.query_json::<ConversationSummary>("SELECT payload FROM conversation_summaries WHERE conversation_id=?1 AND status IN ('queued','running')",[id.to_string()])? {
            s.status=SummaryStatus::Interrupted; s.error=Some(reason.into()); s.revision=s.revision.next()?; s.updated_at=self.now; self.save_summary(&s)?;
        }
        Ok(())
    }
    pub(super) fn recover_summaries(&self) -> Result<()> {
        for mut s in self.query_json::<ConversationSummary>(
            "SELECT payload FROM conversation_summaries WHERE status IN ('queued','running')",
            [],
        )? {
            s.status = SummaryStatus::Interrupted;
            s.error = Some("应用退出时摘要尚未完成，可手动重试".into());
            s.revision = s.revision.next()?;
            s.updated_at = self.now;
            self.save_summary(&s)?;
        }
        Ok(())
    }
}
