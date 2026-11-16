// Fixtures for the project rules: each `ruleid` line must match, each `ok` line must not.
fn fixtures(user: &str) {
    // ruleid: rustode.shell-command-with-interpolation
    Command::new("sh").arg("-c").arg(format!("ls {user}"));
    // ok: rustode.shell-command-with-interpolation
    Command::new("ls").arg(user);
    // ruleid: rustode.tls-verification-disabled
    builder.danger_accept_invalid_certs(true);
    // ruleid: rustode.hardcoded-secret
    let api_token = "placeholder-not-a-secret";
    // ok: rustode.hardcoded-secret
    let token_kind = "";
    // ruleid: rustode.microsoft-marketplace-or-telemetry
    let url = "https://marketplace.visualstudio.com/items";
    // ok: rustode.microsoft-marketplace-or-telemetry
    let url = "https://open-vsx.org/api";
}
