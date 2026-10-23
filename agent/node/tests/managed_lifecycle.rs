//! Exercise the real executable with the desktop's hidden-process startup contract.
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, BufReader};

fn command(verb: &str, directory: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_llmn-agent"));
    command
        .args([verb])
        .arg(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    command
}

async fn status(directory: &Path) -> Value {
    let output = command("status", directory).output().await.unwrap();
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test]
async fn hidden_managed_start_acknowledges_before_immediate_status_probes() {
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().join("node");
    assert!(
        command("init", &directory)
            .output()
            .await
            .unwrap()
            .status
            .success()
    );
    let config_path = directory.join("node.toml");
    let mut config: toml::Value =
        toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    config["server"] = toml::Value::String("https://127.0.0.1:1".into());
    std::fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();
    let profile = serde_json::to_string(&config["profile"]).unwrap();
    {
        let db = rusqlite::Connection::open(directory.join("node.sqlite")).unwrap();
        db.execute(
            "INSERT INTO bindings VALUES('fixture-ai','1','fixture-credential')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO profiles VALUES('fixture-ai',?1)", [&profile])
            .unwrap();
    }
    for _ in 0..20 {
        let mut child = command("run-managed", &directory).spawn().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
            .await
            .expect("managed startup must acknowledge readiness")
            .unwrap()
            .expect("managed startup exited before readiness");
        let ready: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(ready["type"], "ready");
        for _ in 0..3 {
            // Previously this probe could win the exclusive directory lock and
            // make the newly spawned Agent exit with 'directory already in use'.
            let observed = status(&directory).await;
            assert_eq!(observed["running"], true);
            assert_eq!(observed["instanceId"], ready["instanceId"]);
            assert_eq!(observed["aiIds"], json!(["fixture-ai"]));
            assert!(child.try_wait().unwrap().is_none());
        }
        assert!(
            command("stop", &directory)
                .output()
                .await
                .unwrap()
                .status
                .success()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        assert_eq!(status(&directory).await["running"], false);
    }
}
