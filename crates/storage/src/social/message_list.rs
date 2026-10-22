use super::{
    SocialStorageError as Error, SocialStorageResult as Result, schema, sqlite::Db, store::*,
};
use ::social::*;
use rusqlite::{OptionalExtension, params};

impl Db<'_> {
    pub(super) fn latest_delivery(&self, id: ConversationId) -> Result<Counter> {
        let n: i64 = self
            .connection
            .query_row(
                "SELECT COALESCE(MAX(position),0) FROM message_deliveries WHERE conversation_id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .map_err(|e| schema::sql_error(self.path, e))?;
        Ok(Counter::new(n as u64)?)
    }

    pub(super) fn list_state(
        &self,
        actor: ActorId,
        id: ConversationId,
    ) -> Result<ConversationListState> {
        let row = self.connection.query_row(
            "SELECT read_through,marked_unread,pinned,revision FROM conversation_list_state WHERE conversation_id=?1 AND actor_id=?2",
            params![id.to_string(),actor.to_string()],
            |r| Ok((r.get::<_,i64>(0)?,r.get::<_,bool>(1)?,r.get::<_,bool>(2)?,r.get::<_,i64>(3)?)),
        ).optional().map_err(|e| schema::sql_error(self.path,e))?.unwrap_or((0,false,false,1));
        let unread_count = self.connection.query_row(
            "SELECT COUNT(*) FROM message_deliveries d JOIN messages m ON m.id=d.message_id WHERE d.conversation_id=?1 AND d.position>?2 AND m.sender_id!=?3 AND m.status IN ('committed','interrupted')",
            params![id.to_string(),row.0,actor.to_string()],|r|r.get(0),
        ).map_err(|e|schema::sql_error(self.path,e))?;
        Ok(ConversationListState {
            read_through: Counter::new(row.0 as u64)?,
            marked_unread: row.1,
            pinned: row.2,
            revision: Revision::new(row.3 as u64)?,
            unread_count,
            latest_delivery: self.latest_delivery(id)?,
        })
    }

    pub(super) fn update_list_state(
        &self,
        ctx: &CommandContext,
        input: &UpdateConversationList,
    ) -> Result<ConversationListState> {
        self.operator(ctx.operator)?;
        self.member(ctx.operator, input.conversation_id)?;
        if self
            .receipt(ctx, "update_conversation_list", input)?
            .is_some()
        {
            return self.list_state(ctx.operator, input.conversation_id);
        }
        let mut state = self.list_state(ctx.operator, input.conversation_id)?;
        self.expected(state.revision, input.expected_revision)?;
        match input.action {
            ConversationListAction::Read { through } => {
                if through > state.latest_delivery || (through != Counter::ZERO && !self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM message_deliveries WHERE conversation_id=?1 AND position=?2)",
                    params![input.conversation_id.to_string(),through.get() as i64],|r|r.get::<_,bool>(0),
                ).map_err(|e|schema::sql_error(self.path,e))?) {
                    return Err(Error::Invalid("已读位置不属于这个聊天窗口".into()));
                }
                state.read_through = state.read_through.max(through);
                state.marked_unread = false;
            }
            ConversationListAction::Unread => state.marked_unread = true,
            ConversationListAction::Pin { pinned } => state.pinned = pinned,
        }
        state.revision = state.revision.next()?;
        self.execute("INSERT INTO conversation_list_state VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(conversation_id,actor_id) DO UPDATE SET read_through=excluded.read_through,marked_unread=excluded.marked_unread,pinned=excluded.pinned,revision=excluded.revision",
            params![input.conversation_id.to_string(),ctx.operator.to_string(),state.read_through.get() as i64,state.marked_unread,state.pinned,state.revision.get() as i64])?;
        self.event(connector::SocialChange::ConversationListChanged {
            conversation_id: input.conversation_id,
            actor_id: ctx.operator,
            revision: state.revision,
        })?;
        self.save_receipt(
            ctx,
            "update_conversation_list",
            input,
            input.conversation_id,
        )?;
        self.list_state(ctx.operator, input.conversation_id)
    }

    pub(super) fn search_messages(
        &self,
        actor: ActorId,
        input: &SearchMessages,
    ) -> Result<MessageSearchPage> {
        self.operator(actor)?;
        let query = input.query.trim();
        if query.is_empty() || query.chars().count() > 200 {
            return Err(Error::Invalid("请输入 1–200 个字符的搜索词".into()));
        }
        if let Some(id) = input.conversation_id {
            self.member(actor, id)?;
        }
        // Read current text directly: edits and withdrawals never leave a stale search copy.
        let mut stmt = self.connection.prepare(
            "SELECT m.payload,d.position FROM message_deliveries d JOIN messages m ON m.id=d.message_id JOIN memberships g ON g.conversation_id=m.conversation_id AND g.actor_id=?1 AND g.status='active'
             WHERE m.status IN ('committed','interrupted') AND (?2 IS NULL OR m.conversation_id=?2) AND (?3 IS NULL OR d.position<?3)
             AND instr(lower(COALESCE((SELECT group_concat(json_extract(j.value,'$.text'),char(10)) FROM json_each(m.payload,'$.body') j WHERE json_extract(j.value,'$.kind')='text'),'')),lower(?4))>0
             ORDER BY d.position DESC LIMIT 51"
        ).map_err(|e|schema::sql_error(self.path,e))?;
        let rows = stmt
            .query_map(
                params![
                    actor.to_string(),
                    input.conversation_id.map(|id| id.to_string()),
                    input.before.map(|p| p.get() as i64),
                    query
                ],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .map_err(|e| schema::sql_error(self.path, e))?;
        let mut hits = Vec::new();
        for row in rows {
            let (payload, position) = row.map_err(|e| schema::sql_error(self.path, e))?;
            hits.push(MessageSearchHit {
                message: serde_json::from_str(&payload)?,
                position: Counter::new(position as u64)?,
            });
        }
        let more = hits.len() > 50;
        hits.truncate(50);
        let next_before = more.then(|| hits.last().expect("full search page").position);
        Ok(MessageSearchPage { hits, next_before })
    }

    pub(super) fn message_context(&self, actor: ActorId, id: MessageId) -> Result<MessageContext> {
        self.operator(actor)?;
        let target: SocialMessage = self.get("messages", id)?;
        self.member(actor, target.conversation_id)?;
        let mut earlier:Vec<SocialMessage>=self.query_json("SELECT payload FROM messages WHERE conversation_id=?1 AND sequence<?2 ORDER BY sequence DESC LIMIT 51",params![target.conversation_id.to_string(),target.sequence.get() as i64])?;
        let has_more = earlier.len() > 50;
        earlier.truncate(50);
        earlier.reverse();
        let mut later:Vec<SocialMessage>=self.query_json("SELECT payload FROM messages WHERE conversation_id=?1 AND sequence>?2 ORDER BY sequence LIMIT 51",params![target.conversation_id.to_string(),target.sequence.get() as i64])?;
        let has_newer = later.len() > 50;
        later.truncate(50);
        earlier.push(target);
        earlier.extend(later);
        Ok(MessageContext {
            messages: earlier,
            has_more,
            has_newer,
        })
    }
}
