//! ely.by sign-in: the OAuth 2.0 device-code flow against `account.ely.by`, then its Minecraft
//! services profile. An ely.by account plays offline-mode and ely.by-compatible servers; it can't
//! join a server that checks sessions with Microsoft (docs/microsoft-account.md).
//!
//! The OAuth access token is also the Minecraft session token (ely.by's Yggdrasil token is the
//! OAuth one), so [`Secrets`] keeps it as `minecraft_token` and the OAuth refresh token as
//! `refresh_token`. There is no ownership question: every ely.by account may play.
//!
//! The client ID is ely.by's registration of this launcher as a desktop application: the
//! `ELYBY_CLIENT_ID` environment variable, then the launcher's settings value, then
//! [`DEFAULT_CLIENT_ID`].
use super::*;

/// The launcher's ely.by application, registered as a desktop application on ely.by.
pub const DEFAULT_CLIENT_ID: &str = "elyhorizon";

/// What the sign-in asks ely.by for: the account, a refresh token, and the right to open a
/// Minecraft session.
pub const SCOPE: &str = "account_info offline_access minecraft_server_session";

/// Said when neither the environment nor the settings name an ely.by application.
pub const NO_CLIENT_ID: &str =
    "No ely.by client ID is configured: set ELYBY_CLIENT_ID (or the ely.by client ID in the \
     launcher's settings).";

/// The configured client ID, trimmed, if any.
fn configured_client_id() -> Option<String> {
    let from_env = std::env::var("ELYBY_CLIENT_ID").ok();
    let from_settings = crate::backend::Settings::load().elyby_client_id;
    from_env
        .into_iter()
        .chain(from_settings)
        .map(|id| id.trim().to_owned())
        .find(|id| !id.is_empty())
}

/// The ely.by application to sign in with: the environment's, the settings', or the launcher's
/// own. Only an explicitly emptied configuration is an error.
pub fn client_id() -> Result<String, String> {
    match configured_client_id() {
        Some(id) => Ok(id),
        // An explicit empty value means "don't sign in to ely.by": say so rather than quietly
        // using the launcher's own registration.
        None if std::env::var_os("ELYBY_CLIENT_ID").is_some() => Err(NO_CLIENT_ID.to_owned()),
        None => Ok(DEFAULT_CLIENT_ID.to_owned()),
    }
}

/// The OAuth tokens: `access` is also the Minecraft session token.
pub struct Tokens {
    access: String,
    refresh: String,
    expires_at: u64,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tokens(<hidden>)")
    }
}

/// The OAuth error codes as messages for the player.
fn oauth_error(v: &Value) -> String {
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => {
            "The sign-in is still waiting for you in the browser.".into()
        }
        Some("slow_down") => "ely.by asked to poll less often.".into(),
        Some("access_denied") => "Sign-in was declined in the browser.".into(),
        Some("expired_token") => "The sign-in code expired. Start again.".into(),
        Some("invalid_grant") => SIGN_IN_AGAIN.into(),
        Some("invalid_client" | "unauthorized_client") => {
            "ely.by refused this launcher's application (client ID).".into()
        }
        Some(other) => format!("ely.by sign-in failed ({other})."),
        None => "ely.by sign-in failed.".into(),
    }
}

/// The page the player types the code at. ely.by answers `http://account.ely.by/code`, and the
/// https page serves the same form: never send the player to a plaintext page.
fn verification_page(v: &Value) -> String {
    str_field(v, "verification_uri")
        .map(|uri| uri.replacen("http://", "https://", 1))
        .unwrap_or_else(|| "https://account.ely.by/code".into())
}

/// Ask ely.by for a code the player types at its verification page.
pub fn device_code(ep: &Endpoints, client_id: &str) -> Result<DeviceCode, String> {
    let (status, v) = send(
        request_to("POST", &format!("{}/devicecode", ep.ely_oauth)),
        Some(Body::Form(vec![("client_id", client_id), ("scope", SCOPE)])),
        "ely.by",
    )?;
    match (
        status,
        str_field(&v, "user_code"),
        str_field(&v, "device_code"),
    ) {
        (200, Some(user_code), Some(device_code)) => Ok(DeviceCode {
            user_code,
            verification_uri: verification_page(&v),
            interval: v
                .get("interval")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .max(1),
            expires_at: now() + v.get("expires_in").and_then(Value::as_u64).unwrap_or(600),
            device_code,
        }),
        _ => Err(oauth_error(&v)),
    }
}

/// One poll of the token endpoint while the player signs in.
fn poll(ep: &Endpoints, client_id: &str, code: &DeviceCode) -> Result<Poll<Tokens>, String> {
    let (status, v) = send(
        request_to("POST", &format!("{}/token", ep.ely_oauth)),
        Some(Body::Form(vec![
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id),
            ("device_code", &code.device_code),
        ])),
        "ely.by",
    )?;
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => return Ok(Poll::Pending),
        Some("slow_down") => return Ok(Poll::SlowDown),
        _ => {}
    }
    tokens(status, &v, None).map(Poll::Done)
}

/// The token endpoint's answer, with the access token's lifetime. A refresh answer may leave the
/// refresh token out (RFC 6749 §6): the one that was used stays valid, so keep it.
fn tokens(status: u16, v: &Value, previous_refresh: Option<&str>) -> Result<Tokens, String> {
    let refresh = str_field(v, "refresh_token").or_else(|| previous_refresh.map(str::to_owned));
    match (status, str_field(v, "access_token"), refresh) {
        (200, Some(access), Some(refresh)) => Ok(Tokens {
            access,
            refresh,
            expires_at: now() + v.get("expires_in").and_then(Value::as_u64).unwrap_or(3600),
        }),
        _ => Err(oauth_error(v)),
    }
}

/// Wait for the player to finish in the browser, polling as asked, until the code expires or
/// `cancelled` says stop.
pub fn wait_for_sign_in(
    ep: &Endpoints,
    client_id: &str,
    code: &DeviceCode,
    cancelled: &dyn Fn() -> bool,
) -> Result<Tokens, String> {
    let mut interval = code.interval;
    loop {
        let until = std::time::Instant::now() + Duration::from_secs(interval);
        while std::time::Instant::now() < until {
            if cancelled() {
                return Err("Sign-in cancelled.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if now() > code.expires_at {
            return Err("The sign-in code expired. Start again.".into());
        }
        match poll(ep, client_id, code)? {
            Poll::Pending => {}
            Poll::SlowDown => interval += 5,
            Poll::Done(tokens) => return Ok(tokens),
        }
    }
}

/// A new access token from the refresh token.
fn refresh(ep: &Endpoints, client_id: &str, refresh: &str) -> Result<Tokens, String> {
    let (status, v) = send(
        request_to("POST", &format!("{}/token", ep.ely_oauth)),
        Some(Body::Form(vec![
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh),
        ])),
        "ely.by",
    )?;
    tokens(status, &v, Some(refresh))
}

/// From the OAuth tokens to a ready account: the Minecraft services profile.
pub fn finish_sign_in(
    ep: &Endpoints,
    tokens: Tokens,
    progress: &dyn Fn(session::Phase),
) -> Result<(Account, Secrets), String> {
    progress(session::Phase::ElyBy);
    progress(session::Phase::Profile);
    let account = super::profile(ep, &tokens.access, Provider::ElyBy)?.ok_or(NO_PROFILE)?;
    let secrets = Secrets {
        refresh_token: tokens.refresh,
        sign_in_required: false,
        minecraft_token: tokens.access,
        minecraft_expires_at: tokens.expires_at,
        xuid: None,
        // ely.by has no ownership check: every account it signs in may play.
        owns: true,
    };
    Ok((account, secrets))
}

/// Asks ely.by again for the account's profile, renewing the access token first when it has less
/// than an hour left. No ownership question: `owns` stays true.
pub(super) fn check_with(
    ep: &Endpoints,
    client_id: &str,
    account: &Account,
    mut secrets: Secrets,
    force: bool,
    progress: &dyn Fn(session::Phase),
) -> Check {
    if secrets.sign_in_required {
        return Check::NoAnswer(SIGN_IN_AGAIN.into(), secrets);
    }
    if force || secrets.minecraft_expires_at <= now() + RENEW_BEFORE {
        progress(session::Phase::ElyBy);
        let renewed = refresh(ep, client_id, &secrets.refresh_token);
        let tokens = match renewed {
            Ok(tokens) => tokens,
            Err(e) => {
                secrets.sign_in_required = e == SIGN_IN_AGAIN;
                return Check::NoAnswer(e, secrets);
            }
        };
        // ely.by can rotate its refresh credential even if the profile call then fails.
        secrets.refresh_token = tokens.refresh;
        secrets.minecraft_token = tokens.access;
        secrets.minecraft_expires_at = tokens.expires_at;
    }
    progress(session::Phase::Profile);
    match super::profile(ep, &secrets.minecraft_token, Provider::ElyBy) {
        Err(e) => Check::NoAnswer(e, secrets),
        Ok(None) => Check::NoAnswer(NO_PROFILE.into(), secrets),
        Ok(Some(p)) if p.uuid != account.uuid => Check::NoAnswer(
            OTHER_PROFILE.into(),
            Secrets {
                sign_in_required: true,
                ..secrets
            },
        ),
        Ok(Some(p)) => Check::Owns(p, secrets),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verification_page_is_never_plaintext() {
        let answered = json!({"verification_uri": "http://account.ely.by/code"});
        assert_eq!(verification_page(&answered), "https://account.ely.by/code");
        let already_secure = json!({"verification_uri": "https://account.ely.by/code"});
        assert_eq!(
            verification_page(&already_secure),
            "https://account.ely.by/code"
        );
        assert_eq!(
            verification_page(&json!({})),
            "https://account.ely.by/code",
            "no page named: the default is the secure one"
        );
    }

    #[test]
    fn errors_name_ely_by() {
        let v = json!({"error": "authorization_pending"});
        assert!(oauth_error(&v).contains("browser"));
        assert!(oauth_error(&json!({"error": "invalid_client"})).contains("client ID"));
        assert_eq!(
            oauth_error(&json!({"error": "invalid_grant"})),
            SIGN_IN_AGAIN
        );
        assert!(oauth_error(&json!({})).contains("ely.by"));
    }

    #[test]
    fn a_token_answer_keeps_its_lifetime() {
        let v = json!({"access_token": "a", "refresh_token": "r", "expires_in": 120});
        let t = tokens(200, &v, None).unwrap();
        assert_eq!((t.access.as_str(), t.refresh.as_str()), ("a", "r"));
        assert!(t.expires_at >= 120);
        assert!(tokens(400, &json!({"error": "expired_token"}), None).is_err());
        assert_eq!(format!("{t:?}"), "Tokens(<hidden>)");
    }

    #[test]
    fn a_refresh_answer_without_a_new_refresh_token_keeps_the_old_one() {
        let answered = json!({"access_token": "new", "expires_in": 3600});
        let t = tokens(200, &answered, Some("old")).unwrap();
        assert_eq!((t.access.as_str(), t.refresh.as_str()), ("new", "old"));
        assert!(
            tokens(200, &answered, None).is_err(),
            "a sign-in must bring its refresh token"
        );
    }
}
