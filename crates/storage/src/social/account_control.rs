use super::{SocialStorageResult as Result, sqlite::Db, store::*};
use ::social::*;
use events::SocialChange;
use rusqlite::params;

impl Db<'_> {
    pub(super) fn control_account(
        &self,
        ctx: &CommandContext,
        input: &AccountControl,
    ) -> Result<ContactRecord> {
        self.administrator(ctx.operator, input.actor_id)?;
        if self.receipt(ctx, "control_account", input)?.is_some() {
            return self.contact_record(input.actor_id);
        }
        // Only AI accounts have administrator grants and an internal AI profile.
        let mut contact = self.contact_record(input.actor_id)?;
        self.expected(contact.actor.revision, input.expected_revision)?;
        let status = match input.action {
            AccountAction::Archive => EntityStatus::Archived,
            AccountAction::Restore => EntityStatus::Active,
        };
        if contact.actor.status != status {
            contact.actor.status = status;
            contact.actor.revision = contact.actor.revision.next()?;
            self.execute(
                "UPDATE actors SET status=?2,revision=?3,payload=?4 WHERE id=?1",
                params![
                    contact.actor.id.to_string(),
                    super::sqlite::label(&status)?,
                    contact.actor.revision.get() as i64,
                    serde_json::to_string(&contact.actor)?
                ],
            )?;
            if status == EntityStatus::Archived {
                for m in self.query_json::<Membership>(
                    "SELECT payload FROM memberships WHERE actor_id=?1",
                    [contact.actor.id.to_string()],
                )? {
                    self.cancel_contacts(
                        m.conversation_id,
                        Some(contact.actor.id),
                        "AI 账号已停用",
                    )?;
                }
                for run in self
                    .query_json::<AiRun>("SELECT payload FROM ai_runs WHERE status='running'", [])?
                {
                    if run.snapshot.scope.actor_id == contact.actor.id {
                        self.terminate(run, RunStatus::Cancelled, None)?;
                    }
                }
                for mut plan in self.query_json::<ParticipationPlan>(
                    "SELECT payload FROM participation_plans WHERE actor_id=?1 AND status='queued'",
                    [contact.actor.id.to_string()],
                )? {
                    plan.status = PlanStatus::Cancelled;
                    plan.revision = plan.revision.next()?;
                    self.save_plan(&plan)?;
                }
            }
            self.event(SocialChange::ActorChanged {
                actor_id: contact.actor.id,
                revision: contact.actor.revision,
            })?;
        }
        self.save_receipt(ctx, "control_account", input, contact.actor.id)?;
        Ok(contact)
    }
}
