//! The secrets the network log leaves out, by name, in URLs, JSON and forms.

use super::request;
use crate::browser::Logs;
use crate::browser::redact::{cut_note, redact_body, redact_url, secret_name};

#[test]
fn secrets_are_known_by_their_names() {
    for name in [
        "password",
        "Password",
        "new_password",
        "password_confirmation",
        "user[password]",
        "passwd",
        "pass",
        "pw",
        "userPwd",
        "api_key",
        "apiKey",
        "X-API-Key",
        "token",
        "access_token",
        "refreshToken",
        "X-CSRF-Token",
        "client_secret",
        "secretAccessKey",
        "Authorization",
        "auth",
        "cookie",
        "Set-Cookie",
        "session",
        "sessionid",
        "PHPSESSID",
        "connect.sid",
        "credentials",
        "X-Amz-Signature",
        "private_key",
        "jwt",
        "otp",
        "pin",
        "cvc",
        "card_number",
    ] {
        assert!(secret_name(name), "{name} holds a secret");
    }
    for name in [
        "email",
        "user",
        "username",
        "name",
        "id",
        "author",
        "authority",
        "max_tokens",
        "input_tokens",
        "token_type",
        "sessions",
        "passport",
        "compass",
        "sidebar",
        "pinned",
        "keyboard",
        "description",
        "",
    ] {
        assert!(!secret_name(name), "{name} holds no secret");
    }
}

/// The values of secret fields read [redacted] wherever they are in JSON, and nothing else
/// changes: not the names, the order, the spaces, nor values that aren't secrets (`true`,
/// `null` and counts stay).
#[test]
fn json_bodies_keep_their_shape_and_lose_their_secrets() {
    let body = r#"{"email": "ana@x.dev", "password": "hunter2", "remember": true}"#;
    assert_eq!(
        redact_body(body),
        r#"{"email": "ana@x.dev", "password": "[redacted]", "remember": true}"#
    );
    // Nested, in arrays, numbers, and a secret's whole object or array.
    let body = r#"[{"user":{"id":7,"api_key":"sk-1","pin":1234,"tokens":3}},{"credentials":{"user":"a","pass":["x",9,null]}}]"#;
    assert_eq!(
        redact_body(body),
        r#"[{"user":{"id":7,"api_key":"[redacted]","pin":"[redacted]","tokens":3}},{"credentials":{"user":"[redacted]","pass":["[redacted]","[redacted]",null]}}]"#
    );
    // A response with a token, pretty-printed: only the value changes.
    let body = "{\n  \"access_token\": \"eyJhbGciOi.x.y\",\n  \"token_type\": \"Bearer\",\n  \"expires_in\": 3600\n}";
    assert_eq!(
        redact_body(body),
        "{\n  \"access_token\": \"[redacted]\",\n  \"token_type\": \"Bearer\",\n  \"expires_in\": 3600\n}"
    );
    // Quotes, braces and names inside strings are text, not JSON.
    let body = r#"{"note":"say \"password\": {x}","secret":"a\"b}c","n":"é"}"#;
    assert_eq!(
        redact_body(body),
        r#"{"note":"say \"password\": {x}","secret":"[redacted]","n":"é"}"#
    );
}

/// The objects of a list repeat their names, which are read once: every object loses its
/// secrets, also past the names the redaction remembers (a hundred more here).
#[test]
fn every_object_of_a_list_loses_its_secrets() {
    let item = |i: usize, token: &str| format!(r#"{{"id":{i},"token":{token},"k{i}":"v{i}"}}"#);
    let body = format!(
        "[{}]",
        (0..100)
            .map(|i| item(i, &format!(r#""t{i}""#)))
            .collect::<Vec<_>>()
            .join(",")
    );
    let redacted = format!(
        "[{}]",
        (0..100)
            .map(|i| item(i, r#""[redacted]""#))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert_eq!(redact_body(&body), redacted);
}

/// The page agent cuts long bodies (`… (5000 chars)`): a cut JSON body is redacted as far as
/// it goes, a secret cut in the middle too, and the note stays.
#[test]
fn bodies_cut_short_are_redacted_too() {
    let body = r#"{"user":"ana","password":"hunter2-and-mo… (5000 chars)"#;
    assert_eq!(
        redact_body(body),
        r#"{"user":"ana","password":"[redacted]"… (5000 chars)"#
    );
    let body = r#"{"items":[1,2],"session":{"id":"s… (9000 chars)"#;
    assert_eq!(
        redact_body(body),
        r#"{"items":[1,2],"session":{"id":"[redacted]"… (9000 chars)"#
    );
    assert_eq!(
        redact_body("a=1&token=abc… (3000 chars)"),
        "a=1&token=[redacted]… (3000 chars)"
    );
    // A note that isn't one is part of the body.
    assert_eq!(cut_note("x… (12 apples)"), ("x… (12 apples)", ""));
}

#[test]
fn forms_and_urls_lose_their_secrets() {
    assert_eq!(
        redact_body("user=ana&password=hunter2&remember=on"),
        "user=ana&password=[redacted]&remember=on"
    );
    // Names as forms write them: `user[password]`, URL-encoded.
    assert_eq!(
        redact_body("user%5Bpassword%5D=x&api%5Fkey=y&q=a+b"),
        "user%5Bpassword%5D=[redacted]&api%5Fkey=[redacted]&q=a+b"
    );
    // FormData as the page agent writes it: values as they are.
    assert_eq!(
        redact_body("name=Ana Lee&client_secret=s 3&file=[file a.png]"),
        "name=Ana Lee&client_secret=[redacted]&file=[file a.png]"
    );
    // A form's value may hold spaces, as FormData's do.
    assert_eq!(redact_body("token=abc is what we send"), "token=[redacted]");
    // Text, HTML and SQL aren't forms.
    for body in [
        "<a href=\"/x?token=abc\">x</a>",
        "select * from t where password = 'x'",
        "plain text",
    ] {
        assert_eq!(redact_body(body), body);
    }
    assert_eq!(
        redact_url("https://api.x.dev/v1/me?api_key=sk-1&page=2"),
        "https://api.x.dev/v1/me?api_key=[redacted]&page=2"
    );
    assert_eq!(
        redact_url("http://localhost:5173/callback#access_token=abc&token_type=bearer"),
        "http://localhost:5173/callback#access_token=[redacted]&token_type=bearer"
    );
    assert_eq!(
        redact_url("https://ana:hunter2@db.x.dev:8443/path?x=1"),
        "https://ana:[redacted]@db.x.dev:8443/path?x=1"
    );
    for url in [
        "http://localhost:5173/assets/app.js",
        "https://x.dev/#/settings",
        "https://ana@x.dev/a?b=c",
        "https://x.dev/search?q=password",
    ] {
        assert_eq!(redact_url(url), url);
    }
}

/// What the network log keeps, and so what browser_network answers, holds no secret.
#[test]
fn the_network_log_keeps_no_secret() {
    let mut logs = Logs::default();
    let mut login = request("http://localhost:5173/api/login?token=t0k", Some(200), None);
    login.method = "POST".into();
    login.request_body = Some(r#"{"email":"ana@x.dev","password":"hunter2"}"#.into());
    login.response_body = Some(r#"{"ok":true,"session":"s3ss10n"}"#.into());
    logs.push_network(login);
    let answer = logs.network_json(None, false, None).to_string();
    for secret in ["t0k", "hunter2", "s3ss10n"] {
        assert!(!answer.contains(secret), "{secret} in {answer}");
    }
    let kept = &logs.network[0];
    assert_eq!(kept.url, "http://localhost:5173/api/login?token=[redacted]");
    assert_eq!(
        kept.request_body.as_deref(),
        Some(r#"{"email":"ana@x.dev","password":"[redacted]"}"#)
    );
    // Filtering by URL still finds it.
    assert_eq!(logs.network_json(Some("login"), false, None)["matching"], 1);
}
