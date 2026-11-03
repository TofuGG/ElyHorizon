//! Offline play without an owner (this fork, horizon-free): with no account that owns Minecraft:
//! Java Edition in the launcher, an offline name still plays, from the CLI and from the MCP
//! server's `play`. Upstream refused both with the owner gate.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The upstream gate's refusal. It must not come back.
const REFUSED: &str = "Sign in with a Microsoft account that owns Minecraft: Java Edition";

/// Fresh homes with one installed (not prepared) version and an offline name, as a player who
/// played offline before the gate has.
fn home(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("horizon-owner-gate-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let version = root.join("launcher/versions/v2026.10.27.01");
    std::fs::create_dir_all(&version).unwrap();
    std::fs::write(version.join(".installed"), "").unwrap();
    std::fs::write(
        root.join("launcher/accounts.json"),
        r#"{"accounts":[],"selected":null,"offline":["Builder_1"],"offline_selected":"Builder_1"}"#,
    )
    .unwrap();
    root
}

fn launcher(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_horizon-launcher"));
    cmd.args(args)
        .env("HORIZON_HOME", root.join("game"))
        .env("HORIZON_LAUNCHER_HOME", root.join("launcher"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HORIZON_LAUNCHER_NO_SELF_UPDATE", "1")
        .env("HORIZON_NO_AUDIO", "1")
        .env("HORIZON_TELEMETRY_URL", "off")
        // Memory only: the test never reads the player's keychain.
        .env("HORIZON_NO_KEYCHAIN", "1")
        .env("HORIZON_MSA_CLIENT_ID", "test-client");
    cmd
}

fn text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
fn the_cli_plays_an_offline_name_without_an_owner() {
    let root = home("cli");
    let out = launcher(&root, &["account", "select", "Builder_1"])
        .output()
        .unwrap();
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(!said.contains(REFUSED), "{said}");
    // The offline name is the one that plays, and it is still there.
    let accounts: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("launcher/accounts.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(accounts["offline_selected"], "Builder_1", "{accounts}");
    assert_eq!(accounts["offline"][0], "Builder_1", "{accounts}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_mcp_server_does_not_refuse_to_play_without_an_owner() {
    let root = home("mcp");
    let mut child = launcher(&root, &["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let requests = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"play","arguments":{}}}"#,
    ];
    let mut stdin = child.stdin.take().unwrap();
    for r in requests {
        writeln!(stdin, "{r}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let reply = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v["id"] == 2)
        .expect("a reply to play");
    // It may fail to start (no prepared game in this home), but not because of an owner gate.
    let said = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(!said.contains(REFUSED), "{said}");
    let _ = std::fs::remove_dir_all(&root);
}
