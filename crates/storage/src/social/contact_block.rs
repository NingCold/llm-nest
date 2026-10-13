use super::{SocialStorageError as Error, SocialStorageResult as Result, sqlite::Db, store::*};
use ::social::*;
use events::SocialChange;
use rusqlite::params;

impl Db<'_> {
    pub(super) fn blocked_pair(&self, a: ActorId, b: ActorId) -> Result<bool> {
        self.connection.query_row("SELECT EXISTS(SELECT 1 FROM address_book WHERE blocked=1 AND ((owner_id=?1 AND account_id=?2) OR (owner_id=?2 AND account_id=?1)))", params![a.to_string(), b.to_string()], |r| r.get(0))
            .map_err(|e| super::schema::sql_error(self.path, e))
    }

    pub(super) fn direct_blocked(&self, c: &Conversation) -> Result<bool> {
        match c.relationship_id {
            Some(id) => {
                let r: Relationship = self.get("relationships", id)?;
                self.blocked_pair(r.participant_a, r.participant_b)
            }
            None => Ok(false),
        }
    }

    pub(super) fn allow_direct_send(&self, c: &Conversation) -> Result<()> {
        if self.direct_blocked(c)? {
            return Err(Error::Forbidden(
                "此私聊已屏蔽，解除屏蔽后才能发送新消息".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn set_contact_block(
        &self,
        ctx: &CommandContext,
        input: &SetContactBlock,
    ) -> Result<ContactEntry> {
        let replay = self.receipt(ctx, "set_contact_block", input)?.is_some();
        let old = self
            .address_book(ctx.operator)?
            .into_iter()
            .find(|e| e.account_id == input.account_id);
        if replay {
            return old.ok_or_else(|| Error::NotFound("contact entry".into()));
        }
        let _: Actor = self.get("actors", input.account_id)?;
        if input.account_id == ctx.operator {
            return Err(Error::Invalid("不能屏蔽自己的账号".into()));
        }
        if old.as_ref().map(|e| e.revision) != input.expected_revision {
            return Err(Error::Conflict("联系人关系已变化，请刷新后重试".into()));
        }
        let changed = old.as_ref().is_some_and(|e| e.blocked) != input.blocked;
        let entry = ContactEntry {
            owner_id: ctx.operator,
            account_id: input.account_id,
            listed: old.as_ref().is_some_and(|e| e.listed),
            blocked: input.blocked,
            revision: match old {
                Some(e) if changed => e.revision.next()?,
                Some(e) => e.revision,
                None => Revision::ONE,
            },
        };
        self.execute("INSERT INTO address_book(owner_id,account_id,listed,revision,blocked) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner_id,account_id) DO UPDATE SET blocked=excluded.blocked,revision=excluded.revision",params![ctx.operator.to_string(),input.account_id.to_string(),entry.listed,entry.revision.get() as i64,entry.blocked])?;
        if changed {
            // A block/unblock cycle must also invalidate a context prepared before it.
            for mut r in self.query_json::<Relationship>("SELECT payload FROM relationships WHERE (participant_a=?1 AND participant_b=?2) OR (participant_a=?2 AND participant_b=?1)",params![ctx.operator.to_string(),input.account_id.to_string()])? {
                r.revision = r.revision.next()?;
                self.execute("UPDATE relationships SET payload=?2 WHERE id=?1",params![r.id.to_string(),serde_json::to_string(&r)?])?;
                if input.blocked {
                    for c in self.query_json::<Conversation>("SELECT payload FROM conversations WHERE relationship_id=?1",[r.id.to_string()])? {
                        let bout = self.query_json::<Bout>("SELECT payload FROM bouts WHERE conversation_id=?1 AND status='open'",[c.id.to_string()])?.into_iter().next();
                        if let Some(b) = bout {
                            self.close_bout(b)?;
                        } else {
                            self.cancel_runs(c.id, RunStatus::Cancelled)?;
                            self.cancel_queued(c.id, None)?;
                        }
                    }
                }
            }
            if input.blocked {
                self.cancel_blocked_contacts(ctx.operator, input.account_id)?;
            }
        }
        self.event(SocialChange::ContactChanged {
            owner_id: ctx.operator,
            account_id: input.account_id,
            revision: entry.revision,
        })?;
        self.save_receipt(ctx, "set_contact_block", input, input.account_id)?;
        Ok(entry)
    }
}
