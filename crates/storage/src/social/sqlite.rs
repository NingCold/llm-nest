use super::{
    LegacyImportResult, LegacyPreview, SocialStorageError as Error, SocialStorageResult as Result,
    schema, store::*,
};
use crate::DataDirectoryGuard;
use ::social::*;
use async_trait::async_trait;
use events::{SocialChange, SocialEvent};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, oneshot};

type Job = Box<dyn FnOnce(&mut Worker) + Send>;
struct Worker {
    connection: Connection,
    path: PathBuf,
    clock: Arc<dyn Clock>,
}
pub struct SqliteSocialStore {
    sender: Mutex<Option<mpsc::Sender<Job>>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    path: PathBuf,
}
impl SqliteSocialStore {
    pub async fn open(guard: Arc<DataDirectoryGuard>, clock: Arc<dyn Clock>) -> Result<Self> {
        let path = guard.dir().join("social.sqlite3");
        let worker_path = path.clone();
        let (sender, mut receiver) = mpsc::channel::<Job>(64);
        let (ready, received) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("llmn-social-store".into())
            .spawn(move || {
                let _guard = guard;
                let connection = match schema::open(&worker_path, &clock.now().to_rfc3339()) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = ready.send(Err(e));
                        return;
                    }
                };
                let mut worker = Worker {
                    connection,
                    path: worker_path,
                    clock,
                };
                // No provider/tool replay: persisted generating messages become interrupted.
                let recovered = worker.transact(|db| db.recover_runs());
                if ready.send(recovered).is_err() {
                    return;
                }
                while let Some(job) = receiver.blocking_recv() {
                    job(&mut worker);
                }
            })
            .map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
        let store = Self {
            sender: Mutex::new(Some(sender)),
            thread: Mutex::new(Some(thread)),
            path,
        };
        match received.await.map_err(|_| Error::WorkerClosed)? {
            Ok(()) => Ok(store),
            Err(e) => {
                store.close().await?;
                Err(e)
            }
        }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Drain accepted work and release SQLite/OS lock before inspecting or removing files.
    pub async fn close(self) -> Result<()> {
        self.shutdown().await
    }
    async fn stop_writer(&self) -> Result<()> {
        self.sender.lock().expect("sender mutex").take();
        let thread = self.thread.lock().expect("thread mutex").take();
        if let Some(thread) = thread {
            tokio::task::spawn_blocking(move || thread.join())
                .await
                .map_err(|_| Error::WorkerClosed)?
                .map_err(|_| Error::WorkerClosed)?;
        }
        Ok(())
    }
    async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Worker) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (send, receive) = oneshot::channel();
        let sender = self
            .sender
            .lock()
            .expect("sender mutex")
            .clone()
            .ok_or(Error::WorkerClosed)?;
        sender
            .send(Box::new(move |db| {
                let _ = send.send(f(db));
            }))
            .await
            .map_err(|_| Error::WorkerClosed)?;
        receive.await.map_err(|_| Error::WorkerClosed)?
    }
}
impl Drop for SqliteSocialStore {
    fn drop(&mut self) {
        self.sender.get_mut().expect("sender mutex").take();
    }
}
impl Worker {
    fn view(&self) -> Db<'_> {
        Db {
            connection: &self.connection,
            path: &self.path,
            now: self.clock.now(),
        }
    }
    fn transact<T>(&mut self, f: impl FnOnce(&Db<'_>) -> Result<T>) -> Result<T> {
        self.transaction(TransactionBehavior::Immediate, f)
    }
    fn read<T>(&mut self, f: impl FnOnce(&Db<'_>) -> Result<T>) -> Result<T> {
        self.transaction(TransactionBehavior::Deferred, f)
    }
    fn transaction<T>(
        &mut self,
        behavior: TransactionBehavior,
        f: impl FnOnce(&Db<'_>) -> Result<T>,
    ) -> Result<T> {
        let now = self.clock.now();
        let tx = self
            .connection
            .transaction_with_behavior(behavior)
            .map_err(|e| schema::sql_error(&self.path, e))?;
        let result = f(&Db {
            connection: &tx,
            path: &self.path,
            now,
        })?;
        tx.commit().map_err(|e| schema::sql_error(&self.path, e))?;
        Ok(result)
    }
}
pub(super) struct Db<'a> {
    pub connection: &'a Connection,
    pub path: &'a Path,
    pub now: chrono::DateTime<chrono::Utc>,
}
pub(super) struct NewMessage {
    pub body: MessageBody,
    pub status: MessageStatus,
    pub id: MessageId,
    pub run_id: Option<RunId>,
    pub reply_to: Option<MessageId>,
    pub mentions: Vec<ActorId>,
}
impl Db<'_> {
    pub fn execute(&self, sql: &str, args: impl rusqlite::Params) -> Result<usize> {
        self.connection
            .execute(sql, args)
            .map_err(|e| schema::sql_error(self.path, e))
    }
    pub fn optional<T: DeserializeOwned>(
        &self,
        table: &str,
        id: impl ToString,
    ) -> Result<Option<T>> {
        // Table names originate only from fixed internal callers, never client input.
        let s: Option<String> = self
            .connection
            .query_row(
                &format!("SELECT payload FROM {table} WHERE id=?1"),
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| schema::sql_error(self.path, e))?;
        s.map(|s| serde_json::from_str(&s).map_err(Error::from))
            .transpose()
    }
    pub fn get<T: DeserializeOwned>(&self, table: &str, id: impl ToString) -> Result<T> {
        let id = id.to_string();
        self.optional(table, &id)?
            .ok_or_else(|| Error::NotFound(format!("{table}/{id}")))
    }
    pub fn query_json<T: DeserializeOwned>(
        &self,
        sql: &str,
        args: impl rusqlite::Params,
    ) -> Result<Vec<T>> {
        let mut stmt = self
            .connection
            .prepare(sql)
            .map_err(|e| schema::sql_error(self.path, e))?;
        let rows = stmt
            .query_map(args, |r| r.get::<_, String>(0))
            .map_err(|e| schema::sql_error(self.path, e))?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(|e| schema::sql_error(self.path, e))?)
                .map_err(Error::from)
        })
        .collect()
    }
    pub fn operator(&self, id: ActorId) -> Result<Actor> {
        let actor: Actor = self.get("actors", id)?;
        if actor.kind != ActorKind::Human || actor.status != EntityStatus::Active {
            return Err(Error::Forbidden("requires the local human".into()));
        }
        Ok(actor)
    }
    pub fn member(&self, actor: ActorId, conversation: ConversationId) -> Result<Membership> {
        self.query_json::<Membership>("SELECT payload FROM memberships WHERE actor_id=?1 AND conversation_id=?2 AND status='active'",params![actor.to_string(),conversation.to_string()])?
            .into_iter().next().ok_or_else(||Error::Forbidden(format!("not an active member of {conversation}")))
    }
    pub(super) fn writable(&self, conversation: &Conversation) -> Result<()> {
        if conversation.legacy || conversation.status != EntityStatus::Active {
            return Err(Error::Invalid("conversation is read-only".into()));
        }
        Ok(())
    }
    pub(super) fn expected(&self, actual: Revision, expected: Revision) -> Result<()> {
        if actual != expected {
            return Err(Error::Conflict(format!(
                "revision is {actual}, expected {expected}"
            )));
        }
        Ok(())
    }
    pub(super) fn receipt(
        &self,
        ctx: &CommandContext,
        kind: &str,
        input: &impl Serialize,
    ) -> Result<Option<String>> {
        self.operator(ctx.operator)?;
        let found: Option<(String,String)> = self.connection.query_row("SELECT digest,entity_id FROM command_receipts WHERE operator_id=?1 AND kind=?2 AND request_id=?3",params![ctx.operator.to_string(),kind,ctx.request_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|schema::sql_error(self.path,e))?;
        if let Some((digest, id)) = found {
            if digest != schema::digest(&serde_json::to_vec(input)?) {
                return Err(Error::Conflict(
                    "request ID reused with different input".into(),
                ));
            }
            return Ok(Some(id));
        }
        Ok(None)
    }
    pub(super) fn save_receipt(
        &self,
        ctx: &CommandContext,
        kind: &str,
        input: &impl Serialize,
        id: impl ToString,
    ) -> Result<()> {
        self.execute(
            "INSERT INTO command_receipts VALUES(?1,?2,?3,?4,?5)",
            params![
                ctx.operator.to_string(),
                kind,
                ctx.request_id.to_string(),
                schema::digest(&serde_json::to_vec(input)?),
                id.to_string()
            ],
        )?;
        Ok(())
    }
    pub fn event(&self, change: SocialChange) -> Result<()> {
        self.execute(
            "INSERT INTO committed_events(payload) VALUES(?1)",
            [serde_json::to_string(&change)?],
        )?;
        Ok(())
    }
    pub fn insert_actor(&self, actor: &Actor) -> Result<()> {
        self.execute(
            "INSERT INTO actors VALUES(?1,?2,?3,?4,?5)",
            params![
                actor.id.to_string(),
                label(&actor.kind)?,
                label(&actor.status)?,
                actor.revision.get() as i64,
                serde_json::to_string(actor)?
            ],
        )?;
        self.event(SocialChange::ActorChanged {
            actor_id: actor.id,
            revision: actor.revision,
        })
    }
    pub fn new_actor(&self, kind: ActorKind, name: String) -> Actor {
        Actor {
            id: ActorId::new(),
            kind,
            display_name: name,
            avatar_ref: None,
            status: EntityStatus::Active,
            revision: Revision::ONE,
            created_at: self.now,
        }
    }
    pub fn insert_ai(
        &self,
        actor: &Actor,
        persona: String,
        binding: Option<&BindingCandidate>,
    ) -> Result<()> {
        self.insert_actor(actor)?;
        let profile = AiProfile {
            actor_id: actor.id,
            persona,
            revision: Revision::ONE,
        };
        self.execute(
            "INSERT INTO ai_profiles VALUES(?1,1,?2)",
            params![actor.id.to_string(), serde_json::to_string(&profile)?],
        )?;
        if let Some(candidate) = binding {
            let binding = ExecutionBinding {
                id: BindingId::new(),
                actor_id: actor.id,
                provider_id: candidate.provider_id.clone(),
                model_id: candidate.model_id.clone(),
                reasoning_effort: candidate.reasoning_effort.clone(),
                revision: Revision::ONE,
            };
            self.execute(
                "INSERT INTO execution_bindings VALUES(?1,1,?2)",
                params![actor.id.to_string(), serde_json::to_string(&binding)?],
            )?;
        }
        Ok(())
    }
    pub fn new_conversation(
        &self,
        kind: ConversationKind,
        relationship_id: Option<RelationshipId>,
        title: String,
        legacy: bool,
    ) -> Conversation {
        Conversation {
            id: ConversationId::new(),
            kind,
            relationship_id,
            title,
            status: if legacy {
                EntityStatus::Archived
            } else {
                EntityStatus::Active
            },
            paused: legacy,
            revision: Revision::ONE,
            context_epoch: Epoch::ONE,
            participation_epoch: Epoch::ONE,
            last_sequence: Sequence::ZERO,
            created_at: self.now,
            legacy,
        }
    }
    pub fn insert_conversation(&self, c: &Conversation, members: &[ActorId]) -> Result<()> {
        self.execute(
            "INSERT INTO conversations VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                c.id.to_string(),
                label(&c.kind)?,
                c.relationship_id.map(|r| r.to_string()),
                label(&c.status)?,
                c.paused,
                c.revision.get() as i64,
                c.context_epoch.get() as i64,
                c.participation_epoch.get() as i64,
                c.last_sequence.get() as i64,
                serde_json::to_string(c)?
            ],
        )?;
        for actor in members {
            let member = Membership {
                conversation_id: c.id,
                actor_id: *actor,
                status: MembershipStatus::Active,
                history_from_seq: Sequence::ONE,
                revision: Revision::ONE,
            };
            self.execute(
                "INSERT INTO memberships VALUES(?1,?2,'active',1,1,?3)",
                params![
                    c.id.to_string(),
                    actor.to_string(),
                    serde_json::to_string(&member)?
                ],
            )?;
        }
        self.event(SocialChange::ConversationChanged {
            conversation_id: c.id,
            revision: c.revision,
        })
    }
    pub fn save_conversation(&self, c: &Conversation) -> Result<()> {
        self.execute("UPDATE conversations SET status=?2,paused=?3,revision=?4,context_epoch=?5,participation_epoch=?6,last_sequence=?7,payload=?8 WHERE id=?1",params![c.id.to_string(),label(&c.status)?,c.paused,c.revision.get() as i64,c.context_epoch.get() as i64,c.participation_epoch.get() as i64,c.last_sequence.get() as i64,serde_json::to_string(c)?])?;
        self.event(SocialChange::ConversationChanged {
            conversation_id: c.id,
            revision: c.revision,
        })
    }
    pub fn allocate_message(
        &self,
        c: &mut Conversation,
        sender: &Actor,
        input: NewMessage,
    ) -> Result<SocialMessage> {
        let NewMessage {
            body,
            status,
            id,
            run_id,
            reply_to,
            mentions,
        } = input;
        c.last_sequence = c.last_sequence.next()?;
        c.revision = c.revision.next()?;
        let message = SocialMessage {
            id,
            conversation_id: c.id,
            sender_id: sender.id,
            sender_name: sender.display_name.clone(),
            sequence: c.last_sequence,
            body,
            reply_to,
            mentions,
            run_id,
            status,
            revision: Revision::ONE,
            created_at: self.now,
        };
        self.execute(
            "INSERT INTO messages VALUES(?1,?2,?3,?4,1,?5,?6,?7,?8)",
            params![
                message.id.to_string(),
                c.id.to_string(),
                sender.id.to_string(),
                message.sequence.get() as i64,
                label(&status)?,
                reply_to.map(|x| x.to_string()),
                run_id.map(|x| x.to_string()),
                serde_json::to_string(&message)?
            ],
        )?;
        self.save_message_revision(&message, sender.id)?;
        self.save_conversation(c)?;
        self.event(SocialChange::MessageChanged {
            conversation_id: c.id,
            message_id: id,
            revision: message.revision,
        })?;
        Ok(message)
    }
    fn save_message_revision(&self, m: &SocialMessage, author: ActorId) -> Result<()> {
        self.execute(
            "INSERT INTO message_revisions VALUES(?1,?2,?3,?4)",
            params![
                m.id.to_string(),
                m.revision.get() as i64,
                author.to_string(),
                serde_json::to_string(m)?
            ],
        )?;
        Ok(())
    }
    pub(super) fn update_message(&self, m: &SocialMessage, author: ActorId) -> Result<()> {
        self.execute(
            "UPDATE messages SET revision=?2,status=?3,payload=?4 WHERE id=?1",
            params![
                m.id.to_string(),
                m.revision.get() as i64,
                label(&m.status)?,
                serde_json::to_string(m)?
            ],
        )?;
        self.save_message_revision(m, author)?;
        self.event(SocialChange::MessageChanged {
            conversation_id: m.conversation_id,
            message_id: m.id,
            revision: m.revision,
        })
    }
    pub(super) fn save_run(&self, run: &AiRun) -> Result<()> {
        self.execute(
            "UPDATE ai_runs SET status=?2,payload=?3 WHERE id=?1",
            params![
                run.id.to_string(),
                label(&run.status)?,
                serde_json::to_string(run)?
            ],
        )?;
        self.event(SocialChange::RunChanged {
            conversation_id: run.snapshot.scope.conversation_id,
            run_id: run.id,
            revision: run.revision,
        })
    }
    pub(super) fn terminate(
        &self,
        mut run: AiRun,
        status: RunStatus,
        body: Option<MessageBody>,
    ) -> Result<AiRun> {
        let mut m: SocialMessage = self.get("messages", run.message_id)?;
        if let Some(body) = body {
            m.body = body;
        }
        m.status = if status == RunStatus::Succeeded {
            MessageStatus::Committed
        } else {
            MessageStatus::Interrupted
        };
        m.revision = m.revision.next()?;
        self.update_message(&m, m.sender_id)?;
        run.status = status;
        run.revision = run.revision.next()?;
        self.save_run(&run)?;
        self.finish_plan_for_run(&run)?;
        Ok(run)
    }
    pub(super) fn cancel_runs(&self, id: ConversationId, status: RunStatus) -> Result<()> {
        for run in self.query_json::<AiRun>(
            "SELECT payload FROM ai_runs WHERE conversation_id=?1 AND status='running'",
            [id.to_string()],
        )? {
            self.terminate(run, status, None)?;
        }
        Ok(())
    }
    fn recover_runs(&self) -> Result<()> {
        for run in
            self.query_json::<AiRun>("SELECT payload FROM ai_runs WHERE status='running'", [])?
        {
            self.terminate(run, RunStatus::Interrupted, None)?;
        }
        Ok(())
    }
    pub(super) fn start_run(
        &self,
        operator: ActorId,
        input: BeginRun,
        reply_to: Option<MessageId>,
    ) -> Result<AiRun> {
        self.validate_snapshot(&input.snapshot, operator)?;
        let mut c: Conversation =
            self.get("conversations", input.snapshot.scope.conversation_id)?;
        self.expected(c.revision, input.expected_revision)?;
        let bout: Bout = match self
            .query_json(
                "SELECT payload FROM bouts WHERE conversation_id=?1 AND status='open'",
                [c.id.to_string()],
            )?
            .into_iter()
            .next()
        {
            Some(b) => b,
            None => {
                let b = Bout {
                    id: BoutId::new(),
                    conversation_id: c.id,
                    status: BoutStatus::Open,
                    revision: Revision::ONE,
                };
                self.execute(
                    "INSERT INTO bouts VALUES(?1,?2,'open',?3)",
                    params![
                        b.id.to_string(),
                        c.id.to_string(),
                        serde_json::to_string(&b)?
                    ],
                )?;
                self.event(SocialChange::BoutChanged {
                    conversation_id: c.id,
                    bout_id: b.id,
                    revision: b.revision,
                })?;
                b
            }
        };
        let id = RunId::new();
        let ai: Actor = self.get("actors", input.snapshot.scope.actor_id)?;
        let m = self.allocate_message(
            &mut c,
            &ai,
            NewMessage {
                body: MessageBody::default(),
                status: MessageStatus::Generating,
                id: MessageId::new(),
                run_id: Some(id),
                reply_to,
                mentions: vec![],
            },
        )?;
        let run = AiRun {
            id,
            message_id: m.id,
            bout_id: bout.id,
            status: RunStatus::Running,
            revision: Revision::ONE,
            snapshot: input.snapshot,
        };
        self.execute(
            "INSERT INTO ai_runs VALUES(?1,?2,?3,?4,'running',?5)",
            params![
                id.to_string(),
                c.id.to_string(),
                m.id.to_string(),
                bout.id.to_string(),
                serde_json::to_string(&run)?
            ],
        )?;
        if let Some(details) = &input.details {
            self.save_details(id, details)?;
        }
        self.event(SocialChange::RunChanged {
            conversation_id: c.id,
            run_id: id,
            revision: run.revision,
        })?;
        Ok(run)
    }
    pub(super) fn validate_snapshot(
        &self,
        snapshot: &ContextSnapshot,
        operator: ActorId,
    ) -> Result<()> {
        let scope = snapshot.scope;
        let ai: Actor = self.get("actors", scope.actor_id)?;
        if ai.kind != ActorKind::Ai || ai.status != EntityStatus::Active {
            return Err(Error::Invalid("target must be an active AI".into()));
        }
        let c: Conversation = self.get("conversations", scope.conversation_id)?;
        let relationship_revision = if let Some(id) = c.relationship_id {
            Some(self.get::<Relationship>("relationships", id)?.revision)
        } else {
            None
        };
        if relationship_revision != snapshot.relationship_revision
            || snapshot.budget.input_token_limit == Counter::ZERO
            || snapshot.budget.reserved_output_tokens == Counter::ZERO
        {
            return Err(Error::Conflict(
                "relationship/budget snapshot is invalid or changed".into(),
            ));
        }
        self.writable(&c)?;
        self.member(operator, c.id)?;
        self.member(ai.id, c.id)?;
        if c.paused || c.participation_epoch != snapshot.participation_epoch {
            return Err(Error::Conflict("participation changed or paused".into()));
        }
        let profiles: Vec<AiProfile> = self.query_json(
            "SELECT payload FROM ai_profiles WHERE actor_id=?1",
            [ai.id.to_string()],
        )?;
        let bindings: Vec<ExecutionBinding> = self.query_json(
            "SELECT payload FROM execution_bindings WHERE actor_id=?1",
            [ai.id.to_string()],
        )?;
        if profiles.first().map(|p| p.revision) != Some(snapshot.profile_revision)
            || bindings.first().map(|b| b.revision) != Some(snapshot.binding_revision)
        {
            return Err(Error::Conflict(
                "profile/binding snapshot unavailable or changed".into(),
            ));
        }
        for d in &snapshot.dependencies {
            if d.actor_id != ai.id && d.actor_id != operator {
                return Err(Error::Forbidden("unexpected visibility dependency".into()));
            }
            let member = self.member(d.actor_id, d.conversation_id)?;
            let c: Conversation = self.get("conversations", d.conversation_id)?;
            if member.revision != d.membership_revision || c.context_epoch != d.context_epoch {
                return Err(Error::Conflict("context dependency changed".into()));
            }
        }
        // Both target members must have a versioned dependency, including empty context.
        self.require_dependency(snapshot, operator, scope.conversation_id)?;
        self.require_dependency(snapshot, ai.id, scope.conversation_id)?;
        for source in &snapshot.sources {
            let SourceRef::Message {
                conversation_id,
                message_id,
                revision,
            } = source
            else {
                return Err(Error::Invalid(
                    "M0 accepts message sources only; memory/summary readers arrive in M2".into(),
                ));
            };
            let m: SocialMessage = self.get("messages", message_id)?;
            if m.conversation_id != *conversation_id
                || m.revision != *revision
                || m.status != MessageStatus::Committed
            {
                return Err(Error::Conflict("source unavailable or revised".into()));
            }
            for actor in [operator, ai.id] {
                self.require_dependency(snapshot, actor, *conversation_id)?;
                if self.member(actor, *conversation_id)?.history_from_seq > m.sequence {
                    return Err(Error::Forbidden("source precedes visible history".into()));
                }
            }
        }
        Ok(())
    }
    fn require_dependency(
        &self,
        snapshot: &ContextSnapshot,
        actor: ActorId,
        c: ConversationId,
    ) -> Result<()> {
        if !snapshot
            .dependencies
            .iter()
            .any(|d| d.actor_id == actor && d.conversation_id == c)
        {
            return Err(Error::Invalid(
                "missing versioned visibility dependency".into(),
            ));
        }
        Ok(())
    }
}
pub(super) fn label(value: &impl Serialize) -> Result<String> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Invalid("expected enum label".into()))
}
fn validate_text(value: &str, max: usize, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > max {
        return Err(Error::Invalid(format!("invalid {field}")));
    }
    Ok(())
}
pub(super) fn validate_body(body: &MessageBody) -> Result<()> {
    if body.is_empty() || serde_json::to_vec(body)?.len() > 8 * 1024 * 1024 {
        return Err(Error::Invalid("empty or oversized message".into()));
    }
    Ok(())
}

#[async_trait]
impl SocialStore for SqliteSocialStore {
    async fn send_message(&self, ctx: CommandContext, input: SendMessage) -> Result<SavedMessage> {
        self.call(move |w| w.transact(|db| db.send_message(&ctx, &input)))
            .await
    }
    async fn change_membership(
        &self,
        ctx: CommandContext,
        input: ChangeMembership,
    ) -> Result<Conversation> {
        self.call(move |w| w.transact(|db| db.change_membership(&ctx, &input)))
            .await
    }
    async fn plans(&self, operator: ActorId, id: ConversationId) -> Result<Vec<ParticipationPlan>> {
        self.call(move |w| w.read(|db| { db.operator(operator)?; db.member(operator, id)?;
            db.query_json("SELECT payload FROM participation_plans WHERE conversation_id=?1 ORDER BY trigger_sequence DESC,ordinal DESC LIMIT 200", [id.to_string()])
        })).await
    }
    async fn next_plan(
        &self,
        operator: ActorId,
        id: ConversationId,
    ) -> Result<Option<ParticipationPlan>> {
        self.call(move |w| {
            w.read(|db| {
                db.operator(operator)?;
                db.member(operator, id)?;
                db.next_plan(id)
            })
        })
        .await
    }
    async fn queued_conversations(&self, operator: ActorId) -> Result<Vec<ConversationId>> {
        self.call(move |w| w.read(|db| {
            db.operator(operator)?;
            let rows:Vec<Conversation> = db.query_json("SELECT c.payload FROM conversations c WHERE EXISTS(SELECT 1 FROM participation_plans p WHERE p.conversation_id=c.id AND p.status='queued') AND EXISTS(SELECT 1 FROM memberships m WHERE m.conversation_id=c.id AND m.actor_id=?1 AND m.status='active')", [operator.to_string()])?;
            Ok(rows.into_iter().map(|c| c.id).collect())
        })).await
    }
    async fn begin_plan(&self, operator: ActorId, plan: PlanId, input: BeginRun) -> Result<AiRun> {
        self.call(move |w| w.transact(|db| db.begin_plan(operator, plan, input)))
            .await
    }
    async fn fail_plan(&self, operator: ActorId, plan: PlanId, failure: String) -> Result<()> {
        self.call(move |w| {
            w.transact(|db| {
                db.operator(operator)?;
                let mut p: ParticipationPlan = db.get("participation_plans", plan)?;
                db.member(operator, p.conversation_id)?;
                if p.status == PlanStatus::Running {
                    return Err(Error::Conflict(
                        "a running plan must finish through its run".into(),
                    ));
                }
                if p.status == PlanStatus::Queued {
                    p.status = PlanStatus::Failed;
                    p.failure = Some(failure);
                    p.revision = p.revision.next()?;
                    db.save_plan(&p)?;
                }
                Ok(())
            })
        })
        .await
    }
    async fn message_page(
        &self,
        operator: ActorId,
        id: ConversationId,
        before: Option<Sequence>,
    ) -> Result<(Vec<SocialMessage>, bool)> {
        self.call(move |w| w.read(|db| {
            db.operator(operator)?;
            let member=db.member(operator,id)?;
            let mut messages:Vec<SocialMessage>=db.query_json("SELECT payload FROM messages WHERE conversation_id=?1 AND sequence>=?2 AND (?3 IS NULL OR sequence<?3) ORDER BY sequence DESC LIMIT 201",params![id.to_string(),member.history_from_seq.get() as i64,before.map(|s|s.get() as i64)])?;
            let more=messages.len()>200;
            messages.truncate(200); messages.reverse();
            Ok((messages,more))
        })).await
    }
    async fn conversation_record(
        &self,
        operator: ActorId,
        id: ConversationId,
    ) -> Result<ConversationRecord> {
        self.call(move |w| {
            w.read(|db| {
                db.operator(operator)?;
                db.conversation_record(operator, db.get("conversations", id)?)
            })
        })
        .await
    }
    async fn overview(&self, operator: ActorId) -> Result<SocialOverview> {
        self.call(move |w| w.read(|db| db.overview(operator))).await
    }
    async fn contact(&self, operator: ActorId, id: ActorId) -> Result<ContactRecord> {
        self.call(move |w| {
            w.read(|db| {
                db.operator(operator)?;
                db.contact_record(id)
            })
        })
        .await
    }
    async fn context_material(
        &self,
        operator: ActorId,
        scope: ContextScope,
        trigger: MessageId,
    ) -> Result<ContextMaterial> {
        self.call(move |w| w.read(|db| db.context_material(operator, scope, trigger)))
            .await
    }
    async fn run(&self, operator: ActorId, id: RunId) -> Result<AiRun> {
        self.call(move |w| w.read(|db| db.authorized_run(operator, id)))
            .await
    }
    async fn run_for_trigger(
        &self,
        operator: ActorId,
        trigger: MessageId,
    ) -> Result<Option<AiRun>> {
        self.call(move |w| w.read(|db| {
            db.operator(operator)?;
            let message: SocialMessage = db.get("messages",trigger)?;
            db.member(operator,message.conversation_id)?;
            let id: Option<String> = db.connection.query_row("SELECT entity_id FROM command_receipts WHERE operator_id=?1 AND kind='begin_run' AND request_id=?2", params![operator.to_string(),trigger.to_string()], |r|r.get(0)).optional().map_err(|e|schema::sql_error(db.path,e))?;
            id.map(|id| db.get("ai_runs",id)).transpose()
        })).await
    }
    async fn run_details(&self, operator: ActorId, id: RunId) -> Result<Option<RunDetails>> {
        self.call(move |w| {
            w.read(|db| {
                db.authorized_run(operator, id)?;
                Ok(db
                    .query_json(
                        "SELECT payload FROM run_details WHERE run_id=?1",
                        [id.to_string()],
                    )?
                    .into_iter()
                    .next())
            })
        })
        .await
    }
    async fn update_ai(
        &self,
        ctx: CommandContext,
        input: UpdateAi,
        id: ActorId,
    ) -> Result<ContactRecord> {
        self.call(move |w| w.transact(|db| {
            if db.receipt(&ctx,"update_ai",&(id,&input))?.is_some() { return db.contact_record(id); }
            let mut record = db.contact_record(id)?;
            if record.actor.status != EntityStatus::Active { return Err(Error::Invalid("AI is archived".into())); }
            db.expected(record.actor.revision,input.expected_revision)?;
            validate_text(&input.display_name,120,"display name")?;
            validate_text(&input.persona,32*1024,"persona")?;
            record.actor.display_name = input.display_name.clone();
            record.actor.revision = record.actor.revision.next()?;
            record.profile.persona = input.persona.clone();
            record.profile.revision = record.profile.revision.next()?;
            db.execute("UPDATE actors SET revision=?2,payload=?3 WHERE id=?1",params![id.to_string(),record.actor.revision.get() as i64,serde_json::to_string(&record.actor)?])?;
            db.execute("UPDATE ai_profiles SET revision=?2,payload=?3 WHERE actor_id=?1",params![id.to_string(),record.profile.revision.get() as i64,serde_json::to_string(&record.profile)?])?;
            if let Some(candidate) = &input.binding {
                validate_text(&candidate.provider_id,120,"provider")?;
                validate_text(&candidate.model_id,240,"model")?;
                let binding = ExecutionBinding { id: record.binding.as_ref().map(|b|b.id).unwrap_or_default(), actor_id:id, provider_id:candidate.provider_id.clone(), model_id:candidate.model_id.clone(), reasoning_effort:candidate.reasoning_effort.clone(), revision: record.binding.as_ref().map(|b|b.revision.next()).transpose()?.unwrap_or(Revision::ONE) };
                db.execute("INSERT INTO execution_bindings VALUES(?1,?2,?3) ON CONFLICT(actor_id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload",params![id.to_string(),binding.revision.get() as i64,serde_json::to_string(&binding)?])?;
            } else { db.execute("DELETE FROM execution_bindings WHERE actor_id=?1",[id.to_string()])?; }
            db.event(SocialChange::ActorChanged { actor_id:id, revision:record.actor.revision })?;
            db.save_receipt(&ctx,"update_ai",&(id,&input),id)?;
            db.contact_record(id)
        })).await
    }
    async fn shutdown(&self) -> Result<()> {
        self.stop_writer().await
    }
    async fn local_human(&self, initial_name: String) -> Result<Actor> {
        self.call(move |w| {
            w.transact(|db| {
                if let Some(actor) = db
                    .query_json("SELECT payload FROM actors WHERE kind='human'", [])?
                    .into_iter()
                    .next()
                {
                    return Ok(actor);
                }
                validate_text(&initial_name, 256, "human name")?;
                let actor = db.new_actor(ActorKind::Human, initial_name);
                db.insert_actor(&actor)?;
                Ok(actor)
            })
        })
        .await
    }
    async fn actor(&self, operator: ActorId, id: ActorId) -> Result<Actor> {
        self.call(move |w| {
            let db = w.view();
            db.operator(operator)?;
            db.get("actors", id)
        })
        .await
    }
    async fn create_ai(&self, ctx: CommandContext, input: CreateAi) -> Result<Actor> {
        self.call(move |w| {
            w.transact(|db| {
                if let Some(id) = db.receipt(&ctx, "create_ai", &input)? {
                    return db.get("actors", id);
                }
                validate_text(&input.display_name, 256, "AI name")?;
                if input.persona.len() > 64 * 1024 {
                    return Err(Error::Invalid("persona too long".into()));
                }
                if let Some(b) = &input.binding {
                    validate_text(&b.provider_id, 256, "provider")?;
                    validate_text(&b.model_id, 256, "model")?;
                }
                let actor = db.new_actor(ActorKind::Ai, input.display_name.clone());
                db.insert_ai(&actor, input.persona.clone(), input.binding.as_ref())?;
                db.save_receipt(&ctx, "create_ai", &input, actor.id)?;
                Ok(actor)
            })
        })
        .await
    }
    async fn open_direct(&self, ctx: CommandContext, ai: ActorId) -> Result<Conversation> {
        self.call(move |w|w.transact(|db|{
            if let Some(id) = db.receipt(&ctx,"open_direct",&ai)? {let c:Conversation=db.get("conversations",id)?;db.member(ctx.operator,c.id)?;return Ok(c);}
            let target: Actor = db.get("actors",ai)?;
            if target.kind!=ActorKind::Ai || target.status!=EntityStatus::Active {return Err(Error::Invalid("requires an active AI".into()));}
            let (a,b) = if ctx.operator.to_string()<ai.to_string() {(ctx.operator,ai)} else {(ai,ctx.operator)};
            let relationship: Relationship = match db.query_json("SELECT payload FROM relationships WHERE participant_a=?1 AND participant_b=?2",params![a.to_string(),b.to_string()])?.into_iter().next() {
                Some(r) => r,
                None => {
                    let r=Relationship{id:RelationshipId::new(),participant_a:a,participant_b:b,revision:Revision::ONE};
                    db.execute("INSERT INTO relationships VALUES(?1,?2,?3,?4)",params![r.id.to_string(),a.to_string(),b.to_string(),serde_json::to_string(&r)?])?;r
                }
            };
            let c = match db.query_json("SELECT payload FROM conversations WHERE relationship_id=?1",[relationship.id.to_string()])?.into_iter().next() {
                Some(c) => c,
                None => {let c=db.new_conversation(ConversationKind::Direct,Some(relationship.id),target.display_name,false);db.insert_conversation(&c,&[ctx.operator,ai])?;c}
            };
            db.save_receipt(&ctx,"open_direct",&ai,c.id)?;
            Ok(c)
        })).await
    }
    async fn create_group(
        &self,
        ctx: CommandContext,
        title: String,
        mut members: Vec<ActorId>,
    ) -> Result<Conversation> {
        self.call(move |w| {
            w.transact(|db| {
                members.push(ctx.operator);
                members.sort_by_key(|a| a.to_string());
                members.dedup();
                let input = (&title, &members);
                if let Some(id) = db.receipt(&ctx, "create_group", &input)? {
                    let c: Conversation = db.get("conversations", id)?;
                    db.member(ctx.operator, c.id)?;
                    return Ok(c);
                }
                validate_text(&title, 1024, "group title")?;
                if !(2..=64).contains(&members.len()) {
                    return Err(Error::Invalid("group needs 2..=64 members".into()));
                }
                for id in &members {
                    let actor: Actor = db.get("actors", id)?;
                    if actor.status != EntityStatus::Active {
                        return Err(Error::Invalid("archived member".into()));
                    }
                }
                let c = db.new_conversation(ConversationKind::Group, None, title.clone(), false);
                db.insert_conversation(&c, &members)?;
                db.save_receipt(&ctx, "create_group", &input, c.id)?;
                Ok(c)
            })
        })
        .await
    }
    async fn conversation(&self, operator: ActorId, id: ConversationId) -> Result<Conversation> {
        self.call(move |w| {
            let db = w.view();
            db.operator(operator)?;
            db.member(operator, id)?;
            db.get("conversations", id)
        })
        .await
    }
    async fn append_message(
        &self,
        ctx: CommandContext,
        input: AppendMessage,
    ) -> Result<SocialMessage> {
        self.call(move |w| {
            w.transact(|db| {
                if let Some(id) = db.receipt(&ctx, "append_message", &input)? {
                    let m: SocialMessage = db.get("messages", id)?;
                    db.member(ctx.operator, m.conversation_id)?;
                    return Ok(m);
                }
                validate_body(&input.body)?;
                let mut c: Conversation = db.get("conversations", input.conversation_id)?;
                db.member(ctx.operator, c.id)?;
                db.writable(&c)?;
                if let Some(expected) = input.expected_revision {
                    db.expected(c.revision, expected)?;
                }
                if let Some(id) = input.reply_to {
                    let m: SocialMessage = db.get("messages", id)?;
                    if m.conversation_id != c.id || m.status == MessageStatus::Withdrawn {
                        return Err(Error::Invalid("invalid reply target".into()));
                    }
                }
                let sender = db.operator(ctx.operator)?;
                let m = db.allocate_message(
                    &mut c,
                    &sender,
                    NewMessage {
                        body: input.body.clone(),
                        status: MessageStatus::Committed,
                        id: MessageId::new(),
                        run_id: None,
                        reply_to: input.reply_to,
                        mentions: vec![],
                    },
                )?;
                db.save_receipt(&ctx, "append_message", &input, m.id)?;
                Ok(m)
            })
        })
        .await
    }
    async fn revise_message(
        &self,
        ctx: CommandContext,
        input: ReviseMessage,
    ) -> Result<SocialMessage> {
        self.call(move |w| {
            w.transact(|db| {
                if let Some(id) = db.receipt(&ctx, "revise_message", &input)? {
                    let m: SocialMessage = db.get("messages", id)?;
                    db.member(ctx.operator, m.conversation_id)?;
                    return Ok(m);
                }
                validate_body(&input.body)?;
                let mut m: SocialMessage = db.get("messages", input.message_id)?;
                db.member(ctx.operator, m.conversation_id)?;
                let mut c: Conversation = db.get("conversations", m.conversation_id)?;
                db.writable(&c)?;
                if m.sender_id != ctx.operator || m.status != MessageStatus::Committed {
                    return Err(Error::Forbidden(
                        "only own committed messages can be revised".into(),
                    ));
                }
                db.expected(m.revision, input.expected_revision)?;
                m.revision = m.revision.next()?;
                m.body = input.body.clone();
                c.context_epoch = c.context_epoch.next()?;
                c.revision = c.revision.next()?;
                db.update_message(&m, ctx.operator)?;
                db.save_conversation(&c)?;
                db.save_receipt(&ctx, "revise_message", &input, m.id)?;
                Ok(m)
            })
        })
        .await
    }
    async fn control_conversation(
        &self,
        ctx: CommandContext,
        input: ConversationControl,
    ) -> Result<Conversation> {
        self.call(move |w| {
            w.transact(|db| {
                if let Some(id) = db.receipt(&ctx, "control_conversation", &input)? {
                    let c: Conversation = db.get("conversations", id)?;
                    db.member(ctx.operator, c.id)?;
                    return Ok(c);
                }
                let mut c: Conversation = db.get("conversations", input.conversation_id)?;
                db.member(ctx.operator, c.id)?;
                if c.legacy {
                    return Err(Error::Invalid(
                        "legacy history cannot be restored as an active chat".into(),
                    ));
                }
                db.expected(c.revision, input.expected_revision)?;
                c.status = input.status;
                c.paused = input.paused;
                c.revision = c.revision.next()?;
                c.participation_epoch = c.participation_epoch.next()?;
                if c.paused || c.status == EntityStatus::Archived {
                    db.cancel_runs(c.id, RunStatus::Cancelled)?;
                    db.cancel_queued(c.id, None)?;
                }
                db.save_conversation(&c)?;
                db.save_receipt(&ctx, "control_conversation", &input, c.id)?;
                Ok(c)
            })
        })
        .await
    }
    async fn messages(
        &self,
        operator: ActorId,
        id: ConversationId,
        after: Sequence,
        limit: u32,
    ) -> Result<Vec<SocialMessage>> {
        self.call(move |w|{let db=w.view();db.operator(operator)?;let member=db.member(operator,id)?;
            db.query_json("SELECT payload FROM messages WHERE conversation_id=?1 AND sequence>?2 AND sequence>=?3 ORDER BY sequence LIMIT ?4",params![id.to_string(),after.get() as i64,member.history_from_seq.get() as i64,limit.clamp(1,200)])
        }).await
    }
    async fn begin_run(&self, ctx: CommandContext, input: BeginRun) -> Result<AiRun> {
        self.call(move |w| {
            w.transact(|db| {
                if let Some(id) = db.receipt(&ctx, "begin_run", &input)? {
                    let run: AiRun = db.get("ai_runs", id)?;
                    db.member(ctx.operator, run.snapshot.scope.conversation_id)?;
                    return Ok(run);
                }
                let run = db.start_run(ctx.operator, input.clone(), None)?;
                db.save_receipt(&ctx, "begin_run", &input, run.id)?;
                Ok(run)
            })
        })
        .await
    }
    async fn checkpoint_run(
        &self,
        operator: ActorId,
        run_id: RunId,
        body: MessageBody,
        details: Option<RunDetails>,
    ) -> Result<()> {
        self.call(move |w| {
            w.transact(|db| {
                db.operator(operator)?;
                let run: AiRun = db.get("ai_runs", run_id)?;
                db.member(operator, run.snapshot.scope.conversation_id)?;
                if run.status != RunStatus::Running {
                    return Err(Error::Conflict("run has ended".into()));
                }
                if serde_json::to_vec(&body)?.len() > 8 * 1024 * 1024 {
                    return Err(Error::Invalid("oversized draft".into()));
                }
                // Drafts do not generate durable token events or a revision for each delta.
                let mut m: SocialMessage = db.get("messages", run.message_id)?;
                m.body = body;
                db.execute(
                    "UPDATE messages SET payload=?2 WHERE id=?1",
                    params![m.id.to_string(), serde_json::to_string(&m)?],
                )?;
                if let Some(details) = &details {
                    db.save_details(run_id, details)?;
                }
                Ok(())
            })
        })
        .await
    }
    async fn finish_run(&self, ctx: CommandContext, input: FinishRun) -> Result<AiRun> {
        self.call(move |w| {
            w.transact(|db| {
                if let Some(id) = db.receipt(&ctx, "finish_run", &input)? {
                    let run: AiRun = db.get("ai_runs", id)?;
                    db.member(ctx.operator, run.snapshot.scope.conversation_id)?;
                    return Ok(run);
                }
                let run: AiRun = db.get("ai_runs", input.run_id)?;
                db.member(ctx.operator, run.snapshot.scope.conversation_id)?;
                db.expected(run.revision, input.expected_revision)?;
                if run.status != RunStatus::Running || !input.status.is_terminal() {
                    return Err(Error::Conflict("invalid run transition".into()));
                }
                if input.status == RunStatus::Succeeded {
                    validate_body(&input.body)?;
                    db.validate_snapshot(&run.snapshot, ctx.operator)?;
                    let bout: Bout = db.get("bouts", run.bout_id)?;
                    if bout.status != BoutStatus::Open {
                        return Err(Error::Conflict("bout ended".into()));
                    }
                } else if serde_json::to_vec(&input.body)?.len() > 8 * 1024 * 1024 {
                    return Err(Error::Invalid("oversized partial answer".into()));
                }
                let run = db.terminate(run, input.status, Some(input.body.clone()))?;
                if let Some(details) = &input.details {
                    db.save_details(run.id, details)?;
                }
                db.save_receipt(&ctx, "finish_run", &input, run.id)?;
                Ok(run)
            })
        })
        .await
    }
    async fn end_bout(
        &self,
        ctx: CommandContext,
        bout_id: BoutId,
        expected_revision: Revision,
    ) -> Result<Bout> {
        self.call(move |w| {
            w.transact(|db| {
                let input = (bout_id, expected_revision);
                if let Some(id) = db.receipt(&ctx, "end_bout", &input)? {
                    let bout: Bout = db.get("bouts", id)?;
                    db.member(ctx.operator, bout.conversation_id)?;
                    return Ok(bout);
                }
                let mut bout: Bout = db.get("bouts", bout_id)?;
                db.member(ctx.operator, bout.conversation_id)?;
                db.expected(bout.revision, expected_revision)?;
                bout.status = BoutStatus::Ended;
                bout.revision = bout.revision.next()?;
                db.cancel_runs(bout.conversation_id, RunStatus::Cancelled)?;
                db.cancel_queued(bout.conversation_id, None)?;
                db.execute(
                    "UPDATE bouts SET status='ended',payload=?2 WHERE id=?1",
                    params![bout.id.to_string(), serde_json::to_string(&bout)?],
                )?;
                db.event(SocialChange::BoutChanged {
                    conversation_id: bout.conversation_id,
                    bout_id,
                    revision: bout.revision,
                })?;
                db.save_receipt(&ctx, "end_bout", &input, bout_id)?;
                Ok(bout)
            })
        })
        .await
    }
    async fn events(
        &self,
        operator: ActorId,
        after: EventCursor,
        limit: u32,
    ) -> Result<Vec<SocialEvent>> {
        self.call(move |w|{
            let db=w.view();db.operator(operator)?;
            let mut statement=db.connection.prepare("SELECT cursor,payload FROM committed_events WHERE cursor>?1 ORDER BY cursor LIMIT ?2").map_err(|e|schema::sql_error(db.path,e))?;
            let rows=statement.query_map(params![after.get() as i64,limit.clamp(1,500)],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).map_err(|e|schema::sql_error(db.path,e))?;
            rows.map(|r|{let (cursor,payload)=r.map_err(|e|schema::sql_error(db.path,e))?;Ok(SocialEvent{cursor:EventCursor::new(cursor as u64)?,change:serde_json::from_str(&payload)?})}).collect()
        }).await
    }
    async fn backup(&self, destination: PathBuf) -> Result<()> {
        self.call(move |w| schema::backup(&w.connection, &destination))
            .await
    }
    async fn preview_legacy(&self) -> Result<LegacyPreview> {
        self.call(|w| {
            super::legacy::preview(
                w.path
                    .parent()
                    .ok_or_else(|| Error::Invalid("missing data directory".into()))?,
            )
        })
        .await
    }
    async fn import_legacy(
        &self,
        operator: ActorId,
        preview: LegacyPreview,
    ) -> Result<LegacyImportResult> {
        self.call(move |w| {
            w.view().operator(operator)?;
            let root = w
                .path
                .parent()
                .ok_or_else(|| Error::Invalid("missing data directory".into()))?
                .to_owned();
            let sources = super::legacy::prepare_import(&root, &preview)?;
            w.transact(|db| super::legacy::import(db, operator, sources))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn actual_writer_enables_foreign_keys_and_full_synchronous() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteSocialStore::open(
            DataDirectoryGuard::acquire(dir.path()).unwrap(),
            Arc::new(SystemClock),
        )
        .await
        .unwrap();
        let settings = store
            .call(|w| {
                Ok((
                    w.connection
                        .pragma_query_value::<i64, _>(None, "foreign_keys", |r| r.get(0))
                        .map_err(|e| schema::sql_error(&w.path, e))?,
                    w.connection
                        .pragma_query_value::<i64, _>(None, "synchronous", |r| r.get(0))
                        .map_err(|e| schema::sql_error(&w.path, e))?,
                ))
            })
            .await
            .unwrap();
        assert_eq!(settings, (1, 2));
        store.close().await.unwrap();
    }
}
