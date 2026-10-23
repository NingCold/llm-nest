use cloud_server::db as sqlx;
use cloud_server::{AppState, auth};
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("启动失败：{error}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("CLOUD_DATABASE_URL").map_err(|_| "缺少 CLOUD_DATABASE_URL")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .map_err(|_| "无法连接 PostgreSQL")?;
    sqlx::migrator()
        .run(&pool)
        .await
        .map_err(|_| "数据库升级失败")?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("invite") => {
            let days = args.get(1).map(|s| s.parse()).transpose()?.unwrap_or(7);
            println!("{}", auth::create_invite(&pool, days).await?);
            return Ok(());
        }
        Some("revoke-invite") => {
            auth::revoke_invite(&pool, args.get(1).ok_or("缺少邀请码")?).await?;
            println!("邀请已撤销");
            return Ok(());
        }
        Some(_) => {
            return Err("用法：cloud-server [invite [有效天数] | revoke-invite 邀请码]".into());
        }
        None => {}
    }
    let public_url = std::env::var("CLOUD_PUBLIC_URL")
        .map_err(|_| "缺少 CLOUD_PUBLIC_URL")?
        .trim_end_matches('/')
        .to_owned();
    let parsed: axum::http::Uri = public_url.parse()?;
    if parsed.authority().is_none()
        || parsed.path() != "/"
        || !(parsed.scheme_str() == Some("https")
            || (parsed.scheme_str() == Some("http")
                && matches!(parsed.host(), Some("localhost" | "127.0.0.1" | "[::1]"))))
    {
        return Err("CLOUD_PUBLIC_URL 须为 HTTPS 站点地址；本机开发可用 HTTP".into());
    }
    let attachments = PathBuf::from(
        std::env::var("CLOUD_ATTACHMENTS_DIR").map_err(|_| "缺少 CLOUD_ATTACHMENTS_DIR")?,
    );
    std::fs::create_dir_all(&attachments)?;
    let attachments = attachments.canonicalize()?;
    let web_dir =
        PathBuf::from(std::env::var("CLOUD_WEB_DIR").unwrap_or_else(|_| "client/web/dist".into()))
            .canonicalize()?;
    if !web_dir.join("index.html").is_file() {
        return Err("请先构建 Web 界面".into());
    }
    let mail = auth::Mailer::from_env()?;
    if attachments.starts_with(&web_dir) || web_dir.starts_with(&attachments) {
        return Err("附件与网页文件须分开存放".into());
    }
    if let Some(outbox) = mail.outbox_path() {
        let outbox = outbox.canonicalize()?;
        if outbox.starts_with(&web_dir)
            || web_dir.starts_with(&outbox)
            || outbox.starts_with(&attachments)
            || attachments.starts_with(&outbox)
        {
            return Err("测试收件箱须使用独立私有目录".into());
        }
    }
    let bind = std::env::var("CLOUD_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    cloud_server::nodes::recover(&pool).await?;
    println!("云端聊天服务已启动：{bind}");
    let maintenance = tokio::spawn(cloud_server::nodes::maintain(pool.clone()));
    let state = Arc::new(AppState {
        relay: Default::default(),
        pool,
        attachments,
        public_url,
        mail,
    });
    let result = axum::serve(listener, cloud_server::router(state, web_dir))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    maintenance.abort();
    let _ = maintenance.await;
    result?;
    Ok(())
}
