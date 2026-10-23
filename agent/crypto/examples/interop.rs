//! Cross-language acceptance fixture. Input/output contain test-only secrets;
//! use a temporary directory, never a daily user's SDK database.
use anyhow::{Result, ensure};
use llmn_crypto::{Device, matrix_sdk_crypto, ruma};
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(args.len() == 3, "interop <input.json> <output.json>");
    let input: Value = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let directory = tempfile::tempdir()?;
    let device = Device::open(
        directory.path(),
        input["account"].as_str().unwrap(),
        "RUST-INTEROP",
        &[17; 32],
    )
    .await?;
    let info = serde_json::from_str(input["mediaEncryptionInfo"].as_str().unwrap())?;
    let ciphertext: Vec<u8> = serde_json::from_value(input["recoveryCiphertext"].clone())?;
    device.import_recovery(&ciphertext, info).await?;
    let keys = serde_json::from_value(input["roomKeys"].clone())?;
    device
        .machine()
        .store()
        .import_exported_room_keys(keys, |_, _| {})
        .await?;
    let opened = device.open_envelope(&input["sealed"]).await?;
    ensure!(
        opened["content"] == input["expected"],
        "WASM -> Rust plaintext mismatch"
    );
    let room = ruma::RoomId::parse(input["sealed"]["roomId"].as_str().unwrap())?;
    device
        .machine()
        .share_room_key(
            &room,
            std::iter::empty::<&ruma::UserId>(),
            matrix_sdk_crypto::EncryptionSettings::default(),
        )
        .await?;
    let scope = room
        .as_str()
        .strip_prefix('!')
        .unwrap()
        .strip_suffix(":llmn")
        .unwrap();
    let mut sealed = json!({"version":1,"roomId":room,"sender":device.machine().user_id(),"deviceId":"RUST-INTEROP","eventId":"$rust-interop.1.final","content":device.seal(scope,"com.llmn.content.v1", &input["expected"]).await?});
    device.sign_envelope(&mut sealed).await?;
    let room_keys = device.machine().store().export_room_keys(|_| true).await?;
    std::fs::write(
        &args[2],
        serde_json::to_vec(&json!({"sealed":sealed,"roomKeys":room_keys}))?,
    )?;
    Ok(())
}
