use super::*;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;

fn release(version: &str) -> serde_json::Value {
    json!({"tag_name": format!("manager-v{version}"), "prerelease": version.contains('-'), "draft": false,
        "body": "Release notes", "assets": []})
}
fn selected(entries: Vec<serde_json::Value>, preview: bool) -> Option<Release> {
    let mut latest = None;
    select_release(entries, preview, "windows", "x86_64", &mut latest);
    latest
}
#[test]
fn compares_numeric_prerelease_and_stable_precedence() {
    let releases = vec![
        release("0.1.0-beta.10"),
        release("0.1.0-beta.7"),
        release("0.1.0-beta.2"),
    ];
    assert_eq!(
        selected(releases, true).unwrap().version.to_string(),
        "0.1.0-beta.10"
    );
    let releases = vec![
        release("0.1.0-beta.10"),
        release("0.1.0"),
        release("0.1.0-rc.1"),
    ];
    assert_eq!(
        selected(releases, true).unwrap().version.to_string(),
        "0.1.0"
    );
    assert!(
        Version::parse("0.1.0+build.2")
            .unwrap()
            .cmp_precedence(&Version::parse("0.1.0+build.10").unwrap())
            .is_eq()
    );
    let releases = vec![
        release("1.9.9"),
        release("1.10.0"),
        release("2.0.0-alpha.1"),
    ];
    assert_eq!(
        selected(releases, false).unwrap().version.to_string(),
        "1.10.0"
    );
}
#[test]
fn filters_channels_drafts_and_unrelated_tags() {
    let mut draft = release("10.0.0");
    draft["draft"] = json!(true);
    let mut github_preview = release("9.0.0");
    github_preview["prerelease"] = json!(true);
    let entries = vec![
        draft,
        github_preview,
        release("2.0.0-beta.10"),
        release("1.0.0"),
        json!({"tag_name":"core-test-v99.0.0", "body":null}),
        release("bad-version"),
    ];
    assert_eq!(
        selected(entries.clone(), false)
            .unwrap()
            .version
            .to_string(),
        "1.0.0"
    );
    assert_eq!(
        selected(entries, true).unwrap().version.to_string(),
        "9.0.0"
    );
    assert!(selected(vec![release("1.0.0-beta.7")], false).is_none());
}
#[test]
fn defaults_channel_from_current_version_and_preserves_explicit_choice() {
    let mut settings = Settings::default();
    assert!(settings.include_prerelease(&Version::parse("0.1.0-beta.7").unwrap()));
    assert!(!settings.include_prerelease(&Version::parse("0.1.0").unwrap()));
    settings.include_prerelease = Some(false);
    assert!(!settings.include_prerelease(&Version::parse("0.1.0-beta.7").unwrap()));
    settings
        .ignored_versions
        .insert("0.1.0-beta.10+build.a".into());
    assert!(settings.is_ignored(&Version::parse("0.1.0-beta.10+build.b").unwrap()));
    assert!(!settings.is_ignored(&Version::parse("0.1.0").unwrap()));
}
#[test]
fn selects_platform_asset_and_rejects_remote_link_injection() {
    let mut entry = release("0.1.0");
    entry["assets"] = json!([
        {"name":"shinki-emu-manager-0.1.0-windows-x86_64-sources.zip", "browser_download_url":"https://evil.invalid/source.zip"},
        {"name":"shinki-emu-manager-manager-0.1.0-windows-x86_64.zip", "browser_download_url":"https://evil.invalid/a.zip"},
        {"name":"shinki-emu-manager-manager-0.1.0-linux-x86_64.zip", "browser_download_url":"https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/manager-v0.1.0/linux.zip"},
        {"name":"shinki-emu-manager-manager-0.1.0-windows-x86_64.zip", "browser_download_url":"https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/manager-v0.1.0/windows.zip"}
    ]);
    assert!(
        selected(vec![entry], true)
            .unwrap()
            .download_url
            .ends_with("/windows.zip")
    );
    for rejected in [
        "http://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v1.0.0",
        "https://github.com.evil/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v1.0.0",
        "https://github.com/evil/ShinkiEMU/releases/tag/manager-v1.0.0",
        "https://github.com/Project-Shinkirou/ShinkiEMU/issues/1",
        "https://user@github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v1.0.0",
        "https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/manager-v1.0.0/%2e%2e",
    ] {
        assert!(!release_url_allowed(rejected), "{rejected}");
    }
}

fn serve(responses: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/releases", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        responses
            .into_iter()
            .map(|response| {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                stream.write_all(response.as_bytes()).unwrap();
                String::from_utf8(request).unwrap()
            })
            .collect()
    });
    (endpoint, thread)
}
fn response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
fn client() -> reqwest::Client {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

#[tokio::test]
async fn offline_never_connects_to_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let error = fetch(
        &client(),
        &format!("http://{}/", listener.local_addr().unwrap()),
        false,
        true,
        "windows",
        "x86_64",
    )
    .await
    .unwrap_err();
    assert_eq!(error.reason, "offline");
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
#[tokio::test]
async fn malformed_remote_data_returns_errors_and_skips_broken_entries() {
    for body in ["not json", "{}", "null"] {
        let (endpoint, worker) = serve(vec![response(body)]);
        assert!(
            fetch(&client(), &endpoint, true, true, "windows", "x86_64")
                .await
                .is_err()
        );
        worker.join().unwrap();
    }
    let body = serde_json::to_string(&vec![
        json!({"tag_name":null}),
        release("bad-version"),
        release("0.1.0-beta.7"),
    ])
    .unwrap();
    let (endpoint, worker) = serve(vec![response(&body)]);
    let result = fetch(&client(), &endpoint, true, true, "windows", "x86_64")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.version.to_string(), "0.1.0-beta.7");
    assert!(
        result
            .download_url
            .ends_with("manager-v0.1.0-beta.7#assets")
    );
    worker.join().unwrap();
}
#[tokio::test]
async fn paginates_past_unrelated_core_releases() {
    let first = serde_json::to_string(&vec![
        json!({"tag_name":"core-example-v1.0.0", "body":null});
        PAGE_SIZE
    ])
    .unwrap();
    let second = serde_json::to_string(&vec![release("0.1.0-beta.7")]).unwrap();
    let (endpoint, worker) = serve(vec![response(&first), response(&second)]);
    assert_eq!(
        fetch(&client(), &endpoint, true, true, "windows", "x86_64")
            .await
            .unwrap()
            .unwrap()
            .version
            .to_string(),
        "0.1.0-beta.7"
    );
    let requests = worker.join().unwrap();
    assert!(requests[0].contains("page=1"));
    assert!(requests[1].contains("page=2"));
}
#[tokio::test]
async fn reports_rate_limit_without_retrying_and_bounds_response() {
    let (endpoint, worker) = serve(vec!["HTTP/1.1 403 Forbidden\r\nX-RateLimit-Reset: 9999999999\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()]);
    assert_eq!(
        fetch(&client(), &endpoint, true, true, "windows", "x86_64")
            .await
            .unwrap_err()
            .retry_at,
        Some(9999999999)
    );
    assert_eq!(worker.join().unwrap().len(), 1);
    let (endpoint, worker) = serve(vec![format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        MAX_RESPONSE_BYTES + 1
    )]);
    assert!(
        fetch(&client(), &endpoint, true, true, "windows", "x86_64")
            .await
            .is_err()
    );
    worker.join().unwrap();
}
