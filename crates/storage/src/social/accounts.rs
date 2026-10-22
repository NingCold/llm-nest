use super::{SocialStorageError as Error, SocialStorageResult as Result, sqlite::Db, store::*};
use ::social::*;
use connector::SocialChange;
use rusqlite::{OptionalExtension, params};

impl Db<'_> {
    pub(super) fn administrator(&self, operator: ActorId, ai: ActorId) -> Result<bool> {
        self.operator(operator)?;
        self.connection
            .query_row(
                "SELECT initial FROM account_admins WHERE ai_id=?1 AND account_id=?2",
                params![ai.to_string(), operator.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| super::schema::sql_error(self.path, e))?
            .ok_or_else(|| Error::Forbidden("requires this AI account's administrator".into()))
    }
    pub(super) fn managed_account(&self, operator: ActorId, ai: ActorId) -> Result<ManagedAccount> {
        let initial = self.administrator(operator, ai)?;
        let mut statement = self.connection.prepare("SELECT account_id,initial FROM account_admins WHERE ai_id=?1 ORDER BY initial DESC,account_id").map_err(|e|super::schema::sql_error(self.path,e))?;
        let administrators = statement
            .query_map([ai.to_string()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
            })
            .map_err(|e| super::schema::sql_error(self.path, e))?
            .map(|r| {
                let (id, initial) = r.map_err(|e| super::schema::sql_error(self.path, e))?;
                Ok(Administrator {
                    account_id: id
                        .parse()
                        .map_err(|_| Error::Invalid("invalid stored administrator ID".into()))?,
                    initial,
                })
            })
            .collect::<Result<_>>()?;
        Ok(ManagedAccount {
            contact: self.contact_record(ai)?,
            administrators,
            is_initial_administrator: initial,
        })
    }
    pub(super) fn address_book(&self, owner: ActorId) -> Result<Vec<ContactEntry>> {
        self.query_json("SELECT json_object('ownerId',owner_id,'accountId',account_id,'listed',json(CASE listed WHEN 1 THEN 'true' ELSE 'false' END),'blocked',json(CASE blocked WHEN 1 THEN 'true' ELSE 'false' END),'revision',CAST(revision AS TEXT)) FROM address_book WHERE owner_id=?1 ORDER BY rowid",[owner.to_string()])
    }
    pub(super) fn save_actor(&self, actor: &Actor) -> Result<()> {
        self.execute(
            "UPDATE actors SET revision=?2,payload=?3 WHERE id=?1",
            params![
                actor.id.to_string(),
                actor.revision.get() as i64,
                serde_json::to_string(actor)?
            ],
        )?;
        self.event(SocialChange::ActorChanged {
            actor_id: actor.id,
            revision: actor.revision,
        })
    }
    pub(super) fn set_contact(
        &self,
        ctx: &CommandContext,
        input: &SetContact,
    ) -> Result<ContactEntry> {
        let replay = self.receipt(ctx, "set_contact", input)?.is_some();
        let old = self
            .address_book(ctx.operator)?
            .into_iter()
            .find(|e| e.account_id == input.account_id);
        if replay {
            return old.ok_or_else(|| Error::NotFound("contact entry".into()));
        }
        let _: Actor = self.get("actors", input.account_id)?;
        if input.account_id == ctx.operator {
            return Err(Error::Invalid(
                "cannot add yourself to your address book".into(),
            ));
        }
        if old.as_ref().map(|e| e.revision) != input.expected_revision {
            return Err(Error::Conflict(
                "address book entry changed; refresh before retrying".into(),
            ));
        }
        let entry = ContactEntry {
            owner_id: ctx.operator,
            account_id: input.account_id,
            listed: input.listed,
            blocked: old.as_ref().is_some_and(|e| e.blocked),
            revision: match old {
                Some(e) if e.listed == input.listed => e.revision,
                Some(e) => e.revision.next()?,
                None => Revision::ONE,
            },
        };
        self.execute("INSERT INTO address_book(owner_id,account_id,listed,revision) VALUES(?1,?2,?3,?4) ON CONFLICT(owner_id,account_id) DO UPDATE SET listed=excluded.listed,revision=excluded.revision",params![ctx.operator.to_string(),input.account_id.to_string(),entry.listed,entry.revision.get() as i64])?;
        self.event(SocialChange::ContactChanged {
            owner_id: ctx.operator,
            account_id: input.account_id,
            revision: entry.revision,
        })?;
        self.save_receipt(ctx, "set_contact", input, input.account_id)?;
        Ok(entry)
    }
    pub(super) fn set_administrator(
        &self,
        ctx: &CommandContext,
        id: ActorId,
        input: &SetAdministrator,
    ) -> Result<ManagedAccount> {
        if !self.administrator(ctx.operator, id)? {
            return Err(Error::Forbidden(
                "only the initial administrator can grant or revoke administrators".into(),
            ));
        }
        if self
            .receipt(ctx, "set_administrator", &(id, input))?
            .is_some()
        {
            return self.managed_account(ctx.operator, id);
        }
        let mut actor: Actor = self.get("actors", id)?;
        self.expected(actor.revision, input.expected_revision)?;
        if input.granted {
            self.operator(input.account_id)?;
        } else {
            let _: Actor = self.get("actors", input.account_id)?;
        }
        if input.account_id == ctx.operator {
            return Err(Error::Invalid(
                "initial administrator cannot remove or replace itself".into(),
            ));
        }
        let affected = if input.granted {
            self.execute(
                "INSERT OR IGNORE INTO account_admins VALUES(?1,?2,0)",
                params![id.to_string(), input.account_id.to_string()],
            )?
        } else {
            self.execute(
                "DELETE FROM account_admins WHERE ai_id=?1 AND account_id=?2 AND initial=0",
                params![id.to_string(), input.account_id.to_string()],
            )?
        };
        if affected != 0 {
            actor.revision = actor.revision.next()?;
            self.save_actor(&actor)?;
        }
        self.save_receipt(ctx, "set_administrator", &(id, input), id)?;
        self.managed_account(ctx.operator, id)
    }
    pub(super) fn update_account(
        &self,
        ctx: &CommandContext,
        id: ActorId,
        input: &UpdateAccount,
    ) -> Result<Actor> {
        let mut account: Actor = self.get("actors", id)?;
        if id != ctx.operator {
            self.administrator(ctx.operator, id)?;
        }
        if self.receipt(ctx, "update_account", &(id, input))?.is_some() {
            return self.get("actors", id);
        }
        self.expected(account.revision, input.expected_revision)?;
        super::sqlite::validate_text(&input.display_name, 120, "display name")?;
        Self::validate_bio(&input.bio)?;
        account.display_name = input.display_name.clone();
        account.bio = input.bio.clone();
        account.revision = account.revision.next()?;
        self.save_actor(&account)?;
        self.save_receipt(ctx, "update_account", &(id, input), id)?;
        Ok(account)
    }
    pub(super) fn validate_bio(bio: &str) -> Result<()> {
        if bio.len() > 4096 {
            return Err(Error::Invalid("public biography exceeds 4096 bytes".into()));
        }
        Ok(())
    }
}
