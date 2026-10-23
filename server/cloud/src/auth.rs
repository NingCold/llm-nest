use crate::db as sqlx;
use crate::{AppError, AppState, Result};
use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString, rand_core::OsRng},
};
use axum::{
    Json, Router,
    extract::{Path as RoutePath, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

static PASSWORD_WORK: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

#[derive(Clone)]
pub struct Mailer {
    outbox: Option<PathBuf>,
    smtp: Option<lettre::AsyncSmtpTransport<lettre::Tokio1Executor>>,
    from: String,
}
impl Mailer {
    #[cfg(test)]
    pub(crate) fn test(outbox: PathBuf) -> Self {
        Self {
            outbox: Some(outbox),
            smtp: None,
            from: "noreply@localhost".into(),
        }
    }
    pub fn from_env() -> Result<Self> {
        let outbox = std::env::var_os("CLOUD_MAIL_OUTBOX").map(PathBuf::from);
        if let Some(dir) = &outbox {
            if !dir.is_absolute() {
                return Err(AppError::bad("邮件测试目录必须为绝对路径"));
            }
            std::fs::create_dir_all(dir)?;
        }
        let from = std::env::var("CLOUD_SMTP_FROM")
            .unwrap_or_else(|_| "LLM Nest <noreply@localhost>".into());
        let smtp = if outbox.is_none() {
            let host = std::env::var("CLOUD_SMTP_HOST")
                .map_err(|_| AppError::bad("请配置 SMTP 或私有邮件测试目录"))?;
            let mut b = lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::relay(&host)
                .map_err(|_| AppError::bad("SMTP 配置无效"))?;
            if let Ok(port) = std::env::var("CLOUD_SMTP_PORT") {
                b = b.port(port.parse().map_err(|_| AppError::bad("SMTP 端口无效"))?);
            }
            if let Ok(user) = std::env::var("CLOUD_SMTP_USERNAME") {
                b = b.credentials(lettre::transport::smtp::authentication::Credentials::new(
                    user,
                    std::env::var("CLOUD_SMTP_PASSWORD")
                        .map_err(|_| AppError::bad("缺少 SMTP 密码"))?,
                ));
            }
            Some(b.build())
        } else {
            None
        };
        Ok(Self { outbox, smtp, from })
    }
    pub fn outbox_path(&self) -> Option<&Path> {
        self.outbox.as_deref()
    }
    async fn send(&self, email: &str, subject: &str, body: String) -> Result<()> {
        if let Some(dir) = &self.outbox {
            let data =
                serde_json::to_vec(&serde_json::json!({"to":email,"subject":subject,"body":body}))
                    .map_err(|_| AppError::internal())?;
            tokio::fs::write(dir.join(format!("{}.json", Uuid::new_v4())), data).await?;
            return Ok(());
        }
        use lettre::AsyncTransport;
        let message = lettre::Message::builder()
            .from(
                self.from
                    .parse()
                    .map_err(|_| AppError::bad("发件地址无效"))?,
            )
            .to(email.parse().map_err(|_| AppError::bad("邮箱无效"))?)
            .subject(subject)
            .body(body)
            .map_err(|_| AppError::internal())?;
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            self.smtp
                .as_ref()
                .ok_or_else(AppError::internal)?
                .send(message),
        )
        .await
        .map_err(|_| AppError::bad("邮件发送超时，请稍后重试"))?
        .map_err(|_| AppError::bad("邮件发送失败，请稍后重试"))?;
        Ok(())
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: Uuid,
    pub name: String,
    pub bio: String,
    pub kind: String,
    pub status: String,
}
#[derive(Clone, Debug)]
pub struct Principal {
    pub id: Uuid,
    pub session_id: Uuid,
}
fn token() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|v| format!("{v:02x}")).collect()
}
fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
fn email(s: &str) -> Result<String> {
    let s = s.trim().to_lowercase();
    if s.len() > 254
        || s.parse::<lettre::message::Mailbox>().is_err()
        || s.contains(['<', '>', ' ', '\r', '\n'])
    {
        return Err(AppError::bad("请输入有效邮箱"));
    }
    Ok(s)
}
fn name(s: &str) -> Result<String> {
    let s = s.trim();
    if s.is_empty() || s.chars().count() > 80 {
        return Err(AppError::bad("昵称须为 1–80 字"));
    }
    Ok(s.into())
}
async fn password(s: String) -> Result<String> {
    if s.len() < 10 || s.len() > 1024 {
        return Err(AppError::bad("密码须为 10–1024 字节"));
    }
    let permit = PASSWORD_WORK
        .acquire()
        .await
        .map_err(|_| AppError::internal())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        Argon2::default()
            .hash_password(s.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|v| v.to_string())
            .map_err(|_| AppError::internal())
    })
    .await
    .map_err(|_| AppError::internal())?
}
async fn throttle(pool: &PgPool, key: String, limit: i32) -> Result<()> {
    sqlx::query("DELETE FROM auth_throttles WHERE started_at < now()-interval '15 minutes'")
        .execute(pool)
        .await?;
    let attempts:i32=sqlx::query_scalar("INSERT INTO auth_throttles(key) VALUES($1) ON CONFLICT(key) DO UPDATE SET attempts=CASE WHEN auth_throttles.started_at < now()-interval '15 minutes' THEN 1 ELSE auth_throttles.attempts+1 END, started_at=CASE WHEN auth_throttles.started_at < now()-interval '15 minutes' THEN now() ELSE auth_throttles.started_at END RETURNING attempts").bind(hash(&key)).fetch_one(pool).await?;
    if attempts > limit {
        return Err(AppError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "rate_limited",
            message: "操作频繁，请 15 分钟后重试".into(),
        });
    }
    Ok(())
}
fn account(r: sqlx::postgres::PgRow) -> Account {
    Account {
        id: r.get("id"),
        name: r.get("name"),
        bio: r.get("bio"),
        kind: r.get("kind"),
        status: r.get("status"),
    }
}
pub async fn authenticate(s: &AppState, h: &HeaderMap) -> Result<Principal> {
    let t = h
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|p| p.trim().strip_prefix("nest_session="))
        })
        .ok_or_else(AppError::unauthorized)?;
    let r=sqlx::query("SELECT id,account_id FROM sessions WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>now()").bind(hash(t)).fetch_optional(&s.pool).await?.ok_or_else(AppError::unauthorized)?;
    Ok(Principal {
        id: r.get("account_id"),
        session_id: r.get("id"),
    })
}
fn cookie(s: &AppState, t: &str, max_age: u32) -> String {
    format!(
        "nest_session={t}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}{}",
        if s.public_url.starts_with("https://") {
            "; Secure"
        } else {
            ""
        }
    )
}
async fn session(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid) -> Result<String> {
    let t = token();
    sqlx::query("INSERT INTO sessions(id,account_id,token_hash,expires_at) VALUES($1,$2,$3,now()+interval '30 days')").bind(Uuid::new_v4()).bind(id).bind(hash(&t)).execute(&mut **tx).await?;
    Ok(t)
}
fn logged(s: &AppState, a: Account, t: String) -> Result<Response> {
    let mut r = Json(serde_json::json!({"account":a})).into_response();
    r.headers_mut().insert(
        header::SET_COOKIE,
        cookie(s, &t, 2592000)
            .parse()
            .map_err(|_| AppError::internal())?,
    );
    Ok(r)
}
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/registration", patch(correct))
        .route("/auth/resend", post(resend))
        .route("/auth/verify", post(verify))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/forgot", post(forgot))
        .route("/auth/reset", post(reset))
        .route("/auth/sessions", get(sessions))
        .route("/auth/sessions/{id}", delete(revoke_session))
        .route("/me", get(me).patch(profile))
}
#[derive(Deserialize)]
struct Register {
    invite: String,
    email: String,
    password: String,
    name: String,
}
async fn register(
    State(s): State<Arc<AppState>>,
    Json(v): Json<Register>,
) -> Result<Json<serde_json::Value>> {
    let e = email(&v.email)?;
    let n = name(&v.name)?;
    throttle(&s.pool, format!("register:{e}"), 5).await?;
    let p = password(v.password).await?;
    let id = Uuid::new_v4();
    let resume = token();
    let verify = token();
    let mut tx = s.pool.begin().await?;
    let inv=sqlx::query("UPDATE invitations SET reservation_id=$2,reserved_until=now()+interval '30 minutes' WHERE code_hash=$1 AND NOT revoked AND consumed_at IS NULL AND expires_at>now() AND (reserved_until IS NULL OR reserved_until<now()) RETURNING id").bind(hash(&v.invite)).bind(id).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::conflict("邀请码无效、已使用或正在注册中"))?;
    if sqlx::query("SELECT 1 FROM credentials WHERE email=$1")
        .bind(&e)
        .fetch_optional(&mut *tx)
        .await?
        .is_some()
    {
        return Err(AppError::conflict("邮箱已注册"));
    }
    sqlx::query("INSERT INTO registrations(id,invitation_id,email,name,password_hash,resume_hash,verify_hash,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,now()+interval '30 minutes')").bind(id).bind(inv.get::<Uuid,_>("id")).bind(&e).bind(n).bind(p).bind(hash(&resume)).bind(hash(&verify)).execute(&mut *tx).await?;
    s.mail
        .send(
            &e,
            "验证邮箱",
            format!(
                "请在 30 分钟内打开：{}/#/verify?token={verify}",
                s.public_url.trim_end_matches('/')
            ),
        )
        .await?;
    tx.commit().await?;
    Ok(Json(
        serde_json::json!({"registrationId":id,"resumeToken":resume}),
    ))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Resume {
    registration_id: Uuid,
    resume_token: String,
    email: Option<String>,
}
async fn correct(
    State(s): State<Arc<AppState>>,
    Json(v): Json<Resume>,
) -> Result<Json<serde_json::Value>> {
    resend_inner(&s, v, true).await
}
async fn resend(
    State(s): State<Arc<AppState>>,
    Json(v): Json<Resume>,
) -> Result<Json<serde_json::Value>> {
    resend_inner(&s, v, false).await
}
async fn resend_inner(s: &AppState, v: Resume, change: bool) -> Result<Json<serde_json::Value>> {
    throttle(&s.pool, format!("resend:{}", v.registration_id), 5).await?;
    let mut tx = s.pool.begin().await?;
    let r=sqlx::query("SELECT r.email,r.invitation_id FROM registrations r JOIN invitations i ON i.id=r.invitation_id WHERE r.id=$1 AND r.resume_hash=$2 AND r.expires_at>now() AND i.reservation_id=r.id AND NOT i.revoked AND i.consumed_at IS NULL AND i.expires_at>now() FOR UPDATE OF r,i").bind(v.registration_id).bind(hash(&v.resume_token)).fetch_optional(&mut *tx).await?.ok_or_else(AppError::unauthorized)?;
    let e = if change {
        email(
            v.email
                .as_deref()
                .ok_or_else(|| AppError::bad("缺少新邮箱"))?,
        )?
    } else {
        r.get("email")
    };
    if sqlx::query("SELECT 1 FROM credentials WHERE email=$1")
        .bind(&e)
        .fetch_optional(&mut *tx)
        .await?
        .is_some()
    {
        return Err(AppError::conflict("邮箱已注册"));
    }
    let t = token();
    sqlx::query("UPDATE registrations SET email=$2,verify_hash=$3 WHERE id=$1")
        .bind(v.registration_id)
        .bind(&e)
        .bind(hash(&t))
        .execute(&mut *tx)
        .await?;
    s.mail
        .send(
            &e,
            "验证邮箱",
            format!(
                "请在注册有效期内打开：{}/#/verify?token={t}",
                s.public_url.trim_end_matches('/')
            ),
        )
        .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"ok":true})))
}
#[derive(Deserialize)]
struct Token {
    token: String,
}
async fn verify(State(s): State<Arc<AppState>>, Json(v): Json<Token>) -> Result<Response> {
    let mut tx = s.pool.begin().await?;
    let r=sqlx::query("SELECT r.* FROM registrations r JOIN invitations i ON i.id=r.invitation_id WHERE r.verify_hash=$1 AND r.expires_at>now() AND i.reservation_id=r.id AND i.reserved_until>now() AND NOT i.revoked AND i.consumed_at IS NULL AND i.expires_at>now() FOR UPDATE OF r,i").bind(hash(&v.token)).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::bad("验证链接无效或已过期"))?;
    let id = Uuid::new_v4();
    let a = account(
        sqlx::query("INSERT INTO accounts(id,name) VALUES($1,$2) RETURNING *")
            .bind(id)
            .bind(r.get::<String, _>("name"))
            .fetch_one(&mut *tx)
            .await?,
    );
    let inserted=sqlx::query("INSERT INTO credentials(account_id,email,password_hash) VALUES($1,$2,$3) ON CONFLICT(email) DO NOTHING").bind(id).bind(r.get::<String,_>("email")).bind(r.get::<String,_>("password_hash")).execute(&mut *tx).await?;
    if inserted.rows_affected() != 1 {
        return Err(AppError::conflict("邮箱已注册"));
    }
    sqlx::query("UPDATE invitations SET consumed_at=now() WHERE id=$1")
        .bind(r.get::<Uuid, _>("invitation_id"))
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM registrations WHERE id=$1")
        .bind(r.get::<Uuid, _>("id"))
        .execute(&mut *tx)
        .await?;
    let t = session(&mut tx, id).await?;
    tx.commit().await?;
    logged(&s, a, t)
}
#[derive(Deserialize)]
struct Login {
    email: String,
    password: String,
}
async fn login(State(s): State<Arc<AppState>>, Json(v): Json<Login>) -> Result<Response> {
    let e = email(&v.email)?;
    throttle(&s.pool, format!("login:{e}"), 15).await?;
    let r=sqlx::query("SELECT a.*,c.password_hash FROM accounts a JOIN credentials c ON c.account_id=a.id WHERE c.email=$1").bind(e).fetch_optional(&s.pool).await?;
    let encoded = r.as_ref().map(|r| r.get::<String, _>("password_hash"));
    let checked_hash = encoded.clone();
    let permit = PASSWORD_WORK
        .acquire()
        .await
        .map_err(|_| AppError::internal())?;
    let valid = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        // Unknown emails still perform the same password verification work.
        let fallback = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$3XlM6h+nlp9fJ3bplKCdYTxA5fIBdC4n7+I/syMa6B0";
        PasswordHash::new(encoded.as_deref().unwrap_or(fallback))
            .map(|h| Argon2::default().verify_password(v.password.as_bytes(), &h).is_ok())
            .unwrap_or(false)
    }).await.map_err(|_| AppError::internal())?;
    if !valid || r.is_none() {
        return Err(AppError::unauthorized());
    }
    let a = account(r.unwrap());
    let mut tx = s.pool.begin().await?;
    let current: String =
        sqlx::query_scalar("SELECT password_hash FROM credentials WHERE account_id=$1 FOR UPDATE")
            .bind(a.id)
            .fetch_one(&mut *tx)
            .await?;
    if Some(current) != checked_hash {
        return Err(AppError::unauthorized());
    }
    let t = token();
    sqlx::query("INSERT INTO sessions(id,account_id,token_hash,expires_at) VALUES($1,$2,$3,now()+interval '30 days')").bind(Uuid::new_v4()).bind(a.id).bind(hash(&t)).execute(&mut *tx).await?;
    tx.commit().await?;
    logged(&s, a, t)
}
async fn logout(State(s): State<Arc<AppState>>, h: HeaderMap) -> Result<Response> {
    let p = authenticate(&s, &h).await?;
    sqlx::query("UPDATE sessions SET revoked_at=now() WHERE id=$1")
        .bind(p.session_id)
        .execute(&s.pool)
        .await?;
    let mut r = StatusCode::NO_CONTENT.into_response();
    r.headers_mut().insert(
        header::SET_COOKIE,
        cookie(&s, "", 0)
            .parse()
            .map_err(|_| AppError::internal())?,
    );
    Ok(r)
}
#[derive(Deserialize)]
struct Email {
    email: String,
}
async fn forgot(
    State(s): State<Arc<AppState>>,
    Json(v): Json<Email>,
) -> Result<Json<serde_json::Value>> {
    let e = email(&v.email)?;
    throttle(&s.pool, format!("forgot:{e}"), 5).await?;
    if let Some(id) =
        sqlx::query_scalar::<_, Uuid>("SELECT account_id FROM credentials WHERE email=$1")
            .bind(&e)
            .fetch_optional(&s.pool)
            .await?
    {
        let t = token();
        sqlx::query("INSERT INTO password_resets(token_hash,account_id,expires_at) VALUES($1,$2,now()+interval '30 minutes')").bind(hash(&t)).bind(id).execute(&s.pool).await?;
        let delivered = s
            .mail
            .send(
                &e,
                "重设密码",
                format!(
                    "请在 30 分钟内打开：{}/#/reset?token={t}",
                    s.public_url.trim_end_matches('/')
                ),
            )
            .await;
        if delivered.is_err() {
            // Keep the public response independent of whether the account exists.
            eprintln!("password reset email delivery failed");
        }
    }
    Ok(Json(serde_json::json!({"ok":true})))
}
#[derive(Deserialize)]
struct Reset {
    token: String,
    password: String,
}
async fn reset(
    State(s): State<Arc<AppState>>,
    Json(v): Json<Reset>,
) -> Result<Json<serde_json::Value>> {
    throttle(&s.pool, format!("reset:{}", hash(&v.token)), 5).await?;
    let p = password(v.password).await?;
    let mut tx = s.pool.begin().await?;
    let id = sqlx::query_scalar::<_, Uuid>(
        "SELECT account_id FROM password_resets WHERE token_hash=$1 AND expires_at>now()",
    )
    .bind(hash(&v.token))
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::bad("重设链接无效或已过期"))?;
    sqlx::query("SELECT account_id FROM credentials WHERE account_id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if sqlx::query(
        "DELETE FROM password_resets WHERE token_hash=$1 AND account_id=$2 AND expires_at>now()",
    )
    .bind(hash(&v.token))
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        != 1
    {
        return Err(AppError::bad("重设链接无效或已过期"));
    }
    sqlx::query("UPDATE credentials SET password_hash=$2 WHERE account_id=$1")
        .bind(id)
        .bind(p)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM password_resets WHERE account_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE sessions SET revoked_at=now() WHERE account_id=$1 AND revoked_at IS NULL")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"ok":true})))
}
async fn me(State(s): State<Arc<AppState>>, h: HeaderMap) -> Result<Json<Account>> {
    let p = authenticate(&s, &h).await?;
    Ok(Json(account(
        sqlx::query("SELECT * FROM accounts WHERE id=$1")
            .bind(p.id)
            .fetch_one(&s.pool)
            .await?,
    )))
}
#[derive(Deserialize)]
struct Profile {
    name: String,
    bio: String,
}
async fn profile(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<Profile>,
) -> Result<Json<Account>> {
    let p = authenticate(&s, &h).await?;
    let n = name(&v.name)?;
    if v.bio.chars().count() > 2000 {
        return Err(AppError::bad("简介最多 2000 字"));
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(731921)")
        .execute(&mut *tx)
        .await?;
    let updated = account(
        sqlx::query("UPDATE accounts SET name=$2,bio=$3 WHERE id=$1 RETURNING *")
            .bind(p.id)
            .bind(n)
            .bind(v.bio)
            .fetch_one(&mut *tx)
            .await?,
    );
    sqlx::query("INSERT INTO cloud_events(owner) SELECT $1::uuid UNION SELECT owner FROM cloud_contacts WHERE target=$1 UNION SELECT target FROM cloud_contacts WHERE owner=$1 UNION SELECT m.account_id FROM cloud_members m JOIN cloud_members own ON own.conversation=m.conversation WHERE own.account_id=$1")
        .bind(p.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(updated))
}
async fn sessions(State(s): State<Arc<AppState>>, h: HeaderMap) -> Result<Json<serde_json::Value>> {
    let p = authenticate(&s, &h).await?;
    let rs=sqlx::query("SELECT id,created_at,expires_at FROM sessions WHERE account_id=$1 AND revoked_at IS NULL AND expires_at>now() ORDER BY created_at DESC").bind(p.id).fetch_all(&s.pool).await?;
    Ok(Json(
        serde_json::json!({"sessions":rs.iter().map(|r|serde_json::json!({"id":r.get::<Uuid,_>("id"),"createdAt":r.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),"expiresAt":r.get::<chrono::DateTime<chrono::Utc>,_>("expires_at"),"current":r.get::<Uuid,_>("id")==p.session_id})).collect::<Vec<_>>()}),
    ))
}
async fn revoke_session(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    RoutePath(id): RoutePath<Uuid>,
) -> Result<StatusCode> {
    let p = authenticate(&s, &h).await?;
    let r = sqlx::query(
        "UPDATE sessions SET revoked_at=now() WHERE id=$1 AND account_id=$2 AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(p.id)
    .execute(&s.pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}
pub async fn create_invite(pool: &PgPool, days: i64) -> Result<String> {
    if !(1..=365).contains(&days) {
        return Err(AppError::bad("邀请有效天数须为 1–365"));
    }
    let t = token();
    sqlx::query(
        "INSERT INTO invitations(id,code_hash,expires_at) VALUES($1,$2,now()+$3*interval '1 day')",
    )
    .bind(Uuid::new_v4())
    .bind(hash(&t))
    .bind(days as f64)
    .execute(pool)
    .await?;
    Ok(t)
}
pub async fn revoke_invite(pool: &PgPool, code: &str) -> Result<()> {
    let r = sqlx::query(
        "UPDATE invitations SET revoked=true WHERE code_hash=$1 AND consumed_at IS NULL",
    )
    .bind(hash(code))
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::not_found());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;
    struct Fixture {
        state: Arc<AppState>,
        admin: PgPool,
        schema: String,
        _dir: tempfile::TempDir,
    }
    impl Fixture {
        async fn new() -> Option<Self> {
            let url = match std::env::var("CLOUD_TEST_DATABASE_URL") {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("CLOUD_TEST_DATABASE_URL absent: PostgreSQL auth test skipped");
                    return None;
                }
            };
            let admin = PgPoolOptions::new()
                .max_connections(2)
                .connect(&url)
                .await
                .unwrap();
            let schema = format!("auth_test_{}", Uuid::new_v4().simple());
            sqlx::query(&format!("CREATE SCHEMA {schema}"))
                .execute(&admin)
                .await
                .unwrap();
            let path = schema.clone();
            let pool = PgPoolOptions::new()
                .max_connections(5)
                .after_connect(move |c, _| {
                    let sql = format!("SET search_path TO {path}");
                    Box::pin(async move {
                        sqlx::query(&sql).execute(c).await?;
                        Ok(())
                    })
                })
                .connect(&url)
                .await
                .unwrap();
            sqlx::raw_sql(concat!(
                include_str!("../migrations/0001_accounts.sql"),
                include_str!("../migrations/0002_chat.sql"),
                include_str!("../migrations/0003_nodes.sql"),
                include_str!("../migrations/0004_groups.sql"),
                include_str!("../migrations/0005_consciousness.sql")
            ))
            .execute(&pool)
            .await
            .unwrap();
            let dir = tempfile::tempdir().unwrap();
            let state = Arc::new(AppState {
                relay: Default::default(),
                pool,
                attachments: dir.path().join("attachments"),
                public_url: "https://nest.test".into(),
                mail: Mailer {
                    outbox: Some(dir.path().to_owned()),
                    smtp: None,
                    from: "noreply@localhost".into(),
                },
            });
            Some(Self {
                state,
                admin,
                schema,
                _dir: dir,
            })
        }
        async fn finish(self) {
            self.state.pool.close().await;
            sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
                .execute(&self.admin)
                .await
                .unwrap();
            self.admin.close().await;
        }
        async fn register(&self, invite: String, email: &str) -> serde_json::Value {
            register(
                State(self.state.clone()),
                Json(Register {
                    invite,
                    email: email.into(),
                    password: "valid-password-123".into(),
                    name: "测试用户".into(),
                }),
            )
            .await
            .unwrap()
            .0
        }
        async fn latest_token(&self, email: &str) -> String {
            let mut result = None;
            let mut newest = std::time::SystemTime::UNIX_EPOCH;
            for f in std::fs::read_dir(self._dir.path()).unwrap() {
                let p = f.unwrap().path();
                if p.extension().and_then(|v| v.to_str()) != Some("json") {
                    continue;
                }
                let modified = std::fs::metadata(&p).unwrap().modified().unwrap();
                let v: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
                if v["to"] == email && modified >= newest {
                    newest = modified;
                    result = Some(
                        v["body"]
                            .as_str()
                            .unwrap()
                            .split("token=")
                            .nth(1)
                            .unwrap()
                            .to_owned(),
                    );
                }
            }
            result.unwrap()
        }
    }
    #[tokio::test]
    async fn invitation_reservation_correction_and_atomic_verification() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let invite = create_invite(&f.state.pool, 7).await.unwrap();
        let r = f.register(invite.clone(), "wrong@example.test").await;
        let old = f.latest_token("wrong@example.test").await;
        let occupied = register(
            State(f.state.clone()),
            Json(Register {
                invite: invite.clone(),
                email: "steal@example.test".into(),
                password: "valid-password-123".into(),
                name: "偷取".into(),
            }),
        )
        .await;
        assert!(occupied.is_err());
        let id = serde_json::from_value(r["registrationId"].clone()).unwrap();
        let resume = r["resumeToken"].as_str().unwrap().to_owned();
        let _ = correct(
            State(f.state.clone()),
            Json(Resume {
                registration_id: id,
                resume_token: resume,
                email: Some("right@example.test".into()),
            }),
        )
        .await
        .unwrap();
        assert!(
            verify(State(f.state.clone()), Json(Token { token: old }))
                .await
                .is_err()
        );
        let t = f.latest_token("right@example.test").await;
        let (a, b) = tokio::join!(
            verify(State(f.state.clone()), Json(Token { token: t.clone() })),
            verify(State(f.state.clone()), Json(Token { token: t }))
        );
        assert_ne!(a.is_ok(), b.is_ok());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM accounts")
                .fetch_one(&f.state.pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM invitations WHERE consumed_at IS NOT NULL"
            )
            .fetch_one(&f.state.pool)
            .await
            .unwrap(),
            1
        );
        assert!(
            register(
                State(f.state.clone()),
                Json(Register {
                    invite,
                    email: "another@example.test".into(),
                    password: "valid-password-123".into(),
                    name: "其他".into()
                })
            )
            .await
            .is_err()
        );
        f.finish().await;
    }
    #[tokio::test]
    async fn password_reset_revokes_sessions_and_token_is_one_use() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let invite = create_invite(&f.state.pool, 7).await.unwrap();
        f.register(invite, "user@example.test").await;
        let t = f.latest_token("user@example.test").await;
        let response = verify(State(f.state.clone()), Json(Token { token: t }))
            .await
            .unwrap();
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(cookie.contains("HttpOnly") && cookie.contains("Secure"));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            cookie.split(';').next().unwrap().parse().unwrap(),
        );
        assert!(authenticate(&f.state, &headers).await.is_ok());
        let _ = forgot(
            State(f.state.clone()),
            Json(Email {
                email: "user@example.test".into(),
            }),
        )
        .await
        .unwrap();
        let t = f.latest_token("user@example.test").await;
        let _ = reset(
            State(f.state.clone()),
            Json(Reset {
                token: t.clone(),
                password: "new-password-456".into(),
            }),
        )
        .await
        .unwrap();
        assert!(authenticate(&f.state, &headers).await.is_err());
        assert!(
            reset(
                State(f.state.clone()),
                Json(Reset {
                    token: t,
                    password: "another-password-789".into()
                })
            )
            .await
            .is_err()
        );
        assert!(
            login(
                State(f.state.clone()),
                Json(Login {
                    email: "user@example.test".into(),
                    password: "valid-password-123".into()
                })
            )
            .await
            .is_err()
        );
        assert!(
            login(
                State(f.state.clone()),
                Json(Login {
                    email: "user@example.test".into(),
                    password: "new-password-456".into()
                })
            )
            .await
            .is_ok()
        );
        f.finish().await;
    }
    #[tokio::test]
    async fn revoked_invite_and_expired_reservation_cannot_verify() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let invite = create_invite(&f.state.pool, 7).await.unwrap();
        f.register(invite.clone(), "user@example.test").await;
        let t = f.latest_token("user@example.test").await;
        revoke_invite(&f.state.pool, &invite).await.unwrap();
        assert!(
            verify(State(f.state.clone()), Json(Token { token: t }))
                .await
                .is_err()
        );
        let invite = create_invite(&f.state.pool, 7).await.unwrap();
        f.register(invite, "expired@example.test").await;
        let t = f.latest_token("expired@example.test").await;
        sqlx::query("UPDATE invitations SET reserved_until=now()-interval '1 second'")
            .execute(&f.state.pool)
            .await
            .unwrap();
        assert!(
            verify(State(f.state.clone()), Json(Token { token: t }))
                .await
                .is_err()
        );
        f.finish().await;
    }
    #[test]
    fn validates_identity_and_tokens() {
        assert!(email("Alice@Example.com").is_ok());
        assert!(email("Alice <alice@example.com>").is_err());
        assert!(name("  ").is_err());
        assert_ne!(token(), token());
        assert_eq!(hash("secret").len(), 64);
    }
    #[tokio::test]
    async fn concurrent_reservations_and_session_ownership() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let invite = create_invite(&f.state.pool, 7).await.unwrap();
        let attempt = |email: &str| Register {
            invite: invite.clone(),
            email: email.into(),
            password: "valid-password-123".into(),
            name: "测试".into(),
        };
        let (a, b) = tokio::join!(
            register(State(f.state.clone()), Json(attempt("a@example.test"))),
            register(State(f.state.clone()), Json(attempt("b@example.test")))
        );
        assert_ne!(a.is_ok(), b.is_ok());
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        for id in [first, second] {
            sqlx::query("INSERT INTO accounts(id,name) VALUES($1,'测试')")
                .bind(id)
                .execute(&f.state.pool)
                .await
                .unwrap();
        }
        let mut tx = f.state.pool.begin().await.unwrap();
        let first_token = session(&mut tx, first).await.unwrap();
        let second_token = session(&mut tx, second).await.unwrap();
        tx.commit().await.unwrap();
        let headers = |t: String| {
            let mut h = HeaderMap::new();
            h.insert(header::COOKIE, format!("nest_session={t}").parse().unwrap());
            h
        };
        let first_headers = headers(first_token);
        let second_headers = headers(second_token);
        let p = authenticate(&f.state, &second_headers).await.unwrap();
        assert!(
            revoke_session(
                State(f.state.clone()),
                first_headers,
                RoutePath(p.session_id)
            )
            .await
            .is_err()
        );
        assert!(authenticate(&f.state, &second_headers).await.is_ok());
        revoke_session(
            State(f.state.clone()),
            second_headers.clone(),
            RoutePath(p.session_id),
        )
        .await
        .unwrap();
        assert!(authenticate(&f.state, &second_headers).await.is_err());
        f.finish().await;
    }
    #[tokio::test]
    async fn profile_update_notifies_related_accounts_atomically() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let owner = Uuid::new_v4();
        let contact = Uuid::new_v4();
        let peer = Uuid::new_v4();
        let unrelated = Uuid::new_v4();
        for id in [owner, contact, peer, unrelated] {
            sqlx::query("INSERT INTO accounts(id,name) VALUES($1,'旧名')")
                .bind(id)
                .execute(&f.state.pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO cloud_contacts(owner,target) VALUES($1,$2)")
            .bind(contact)
            .bind(owner)
            .execute(&f.state.pool)
            .await
            .unwrap();
        let (first, second) = if owner < peer {
            (owner, peer)
        } else {
            (peer, owner)
        };
        sqlx::query("INSERT INTO cloud_conversations(id,first_id,second_id) VALUES($1,$2,$3)")
            .bind(Uuid::new_v4())
            .bind(first)
            .bind(second)
            .execute(&f.state.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO cloud_members(conversation,account_id) SELECT id,first_id FROM cloud_conversations UNION ALL SELECT id,second_id FROM cloud_conversations").execute(&f.state.pool).await.unwrap();
        let mut tx = f.state.pool.begin().await.unwrap();
        let t = session(&mut tx, owner).await.unwrap();
        tx.commit().await.unwrap();
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, format!("nest_session={t}").parse().unwrap());
        let updated = profile(
            State(f.state.clone()),
            h,
            Json(Profile {
                name: "新名".into(),
                bio: "新简介".into(),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(updated.name, "新名");
        let mut notified =
            sqlx::query_scalar::<_, Uuid>("SELECT owner FROM cloud_events ORDER BY owner")
                .fetch_all(&f.state.pool)
                .await
                .unwrap();
        notified.sort();
        let mut expected = vec![owner, contact, peer];
        expected.sort();
        assert_eq!(notified, expected);
        f.finish().await;
    }
}
