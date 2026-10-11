use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::Deserialize;
use serde_json::json;

use super::types::{ProviderAccount, ProviderId, RemoteRepository};
use crate::core::network_proxy::{app_http_client, app_http_client_no_redirects};

pub trait GitProvider: Send + Sync {
    fn validate_token(&self, token: &str) -> Result<ProviderAccount>;
    fn list_repositories(&self, token: &str) -> Result<Vec<RemoteRepository>>;
    fn create_private_repository(&self, token: &str, name: &str) -> Result<RemoteRepository>;
}

pub fn provider(id: ProviderId, proxy_url: &str) -> Result<Box<dyn GitProvider>> {
    Ok(Box::new(ApiProvider::new_with_proxy(
        id,
        provider_base_url(id),
        proxy_url,
        true,
    )?))
}

pub fn oauth_provider(id: ProviderId, proxy_url: &str) -> Result<Box<dyn GitProvider>> {
    Ok(Box::new(ApiProvider::new_with_proxy(
        id,
        provider_base_url(id),
        proxy_url,
        false,
    )?))
}

fn provider_base_url(id: ProviderId) -> &'static str {
    match id {
        ProviderId::Github => "https://api.github.com",
        ProviderId::Gitlab => "https://gitlab.com/api/v4",
        ProviderId::Gitee => "https://gitee.com/api/v5",
    }
}

#[derive(Clone, Debug)]
pub struct ApiProvider {
    id: ProviderId,
    base_url: String,
    client: Client,
}

impl ApiProvider {
    #[cfg(test)]
    pub fn with_base_url(id: ProviderId, base_url: impl Into<String>) -> Self {
        Self::new(id, &base_url.into())
    }

    #[cfg(test)]
    pub fn with_base_url_and_proxy(
        id: ProviderId,
        base_url: impl Into<String>,
        proxy_url: &str,
    ) -> Result<Self> {
        Self::new_with_proxy(id, &base_url.into(), proxy_url, false)
    }

    #[cfg(test)]
    fn new(id: ProviderId, base_url: &str) -> Self {
        Self {
            id,
            base_url: base_url.trim_end_matches('/').to_string(),
            client: app_http_client("", Some(20)).expect("build provider HTTP client"),
        }
    }

    fn new_with_proxy(
        id: ProviderId,
        base_url: &str,
        proxy_url: &str,
        follow_redirects: bool,
    ) -> Result<Self> {
        let client = if follow_redirects {
            app_http_client(proxy_url, Some(20))?
        } else {
            app_http_client_no_redirects(proxy_url, Some(20))?
        };
        Ok(Self {
            id,
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        })
    }

    fn headers(&self, token: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("Skills-Hub"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        match self.id {
            ProviderId::Github => {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {token}"))?,
                );
            }
            ProviderId::Gitlab => {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {token}"))?,
                );
            }
            ProviderId::Gitee => {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {token}"))?,
                );
            }
        }
        Ok(headers)
    }

    fn checked(response: reqwest::blocking::Response) -> Result<reqwest::blocking::Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body = response.text().unwrap_or_default();
        bail!(
            "provider API returned {}: {}",
            status,
            sanitize_message(&body)
        );
    }
}

#[derive(Deserialize)]
struct AccountResponse {
    login: Option<String>,
    username: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct RepositoryResponse {
    name: String,
    html_url: Option<String>,
    web_url: Option<String>,
    clone_url: Option<String>,
    http_url_to_repo: Option<String>,
    ssh_url: Option<String>,
    ssh_url_to_repo: Option<String>,
    private: Option<bool>,
    visibility: Option<String>,
}

impl GitProvider for ApiProvider {
    fn validate_token(&self, token: &str) -> Result<ProviderAccount> {
        if token.trim().is_empty() {
            bail!("token is empty");
        }
        let response = Self::checked(
            self.client
                .get(format!("{}/user", self.base_url))
                .headers(self.headers(token)?)
                .send()
                .context("validate provider token")?,
        )?;
        let account: AccountResponse = response.json().context("decode provider account")?;
        let login = account
            .login
            .or(account.username)
            .context("provider account response missing login")?;
        Ok(ProviderAccount {
            login,
            display_name: account.name,
        })
    }

    fn list_repositories(&self, token: &str) -> Result<Vec<RemoteRepository>> {
        let path = match self.id {
            ProviderId::Github => {
                "/user/repos?visibility=all&affiliation=owner&per_page=100&sort=updated"
            }
            ProviderId::Gitlab => {
                "/projects?membership=true&simple=true&per_page=100&order_by=last_activity_at"
            }
            ProviderId::Gitee => "/user/repos?visibility=all&per_page=100&sort=updated",
        };
        let response = Self::checked(
            self.client
                .get(format!("{}{}", self.base_url, path))
                .headers(self.headers(token)?)
                .send()
                .context("list provider repositories")?,
        )?;
        let repositories: Vec<RepositoryResponse> =
            response.json().context("decode repository list")?;
        repositories.into_iter().map(normalize_repository).collect()
    }

    fn create_private_repository(&self, token: &str, name: &str) -> Result<RemoteRepository> {
        let name = name.trim();
        if name.is_empty() {
            bail!("repository name is empty");
        }
        let (path, payload) = match self.id {
            ProviderId::Github => (
                "/user/repos",
                json!({"name": name, "private": true, "auto_init": true}),
            ),
            ProviderId::Gitlab => (
                "/projects",
                json!({"name": name, "visibility": "private", "initialize_with_readme": true}),
            ),
            ProviderId::Gitee => (
                "/user/repos",
                json!({"name": name, "private": true, "auto_init": true}),
            ),
        };
        let response = Self::checked(
            self.client
                .post(format!("{}{}", self.base_url, path))
                .headers(self.headers(token)?)
                .json(&payload)
                .send()
                .context("create private sync repository")?,
        )?;
        normalize_repository(response.json().context("decode repository response")?)
    }
}

fn normalize_repository(repo: RepositoryResponse) -> Result<RemoteRepository> {
    use super::types::RepositoryVisibility;
    let web_url = repo
        .html_url
        .clone()
        .or_else(|| repo.web_url.clone())
        .unwrap_or_default();
    let visibility = match repo.visibility.as_deref() {
        Some("public") => RepositoryVisibility::Public,
        Some("private") => RepositoryVisibility::Private,
        Some("internal") => RepositoryVisibility::Internal,
        Some(_) => RepositoryVisibility::Unknown,
        None => match repo.private {
            Some(true) => RepositoryVisibility::Private,
            Some(false) => RepositoryVisibility::Public,
            None => RepositoryVisibility::Unknown,
        },
    };
    Ok(RemoteRepository {
        visibility,
        name: repo.name,
        web_url,
        clone_url: repo
            .clone_url
            .or(repo.http_url_to_repo)
            .or(repo.html_url)
            .context("provider response missing HTTPS clone URL")?,
        ssh_url: repo.ssh_url.or(repo.ssh_url_to_repo),
        private: repo.private.unwrap_or_else(|| {
            repo.visibility
                .as_deref()
                .is_some_and(|value| value == "private")
        }),
    })
}

#[cfg(test)]
#[test]
fn missing_or_internal_visibility_is_not_anonymous_public_access() {
    use super::types::RepositoryVisibility;
    for (extra, expected) in [
        ("", RepositoryVisibility::Unknown),
        (
            r#","visibility":"internal""#,
            RepositoryVisibility::Internal,
        ),
        (r#","private":false"#, RepositoryVisibility::Public),
        (r#","visibility":"private""#, RepositoryVisibility::Private),
    ] {
        let json = format!(r#"{{"name":"repo","clone_url":"https://example/repo.git"{extra}}}"#);
        let repo = normalize_repository(serde_json::from_str(&json).unwrap()).unwrap();
        assert_eq!(repo.visibility, expected);
    }
}

fn sanitize_message(value: &str) -> String {
    let compact = value.replace(['\n', '\r'], " ");
    compact.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Duration;

    fn account_proxy_once() -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            request_tx
                .send(String::from_utf8_lossy(&buffer[..read]).into_owned())
                .unwrap();
            let body = r#"{"login":"may","username":"may","name":"May"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            stream.flush().unwrap();
        });
        (format!("http://{address}"), request_rx, handle)
    }

    #[test]
    fn github_validates_account_and_creates_private_repository() {
        let mut server = mockito::Server::new();
        let account = server
            .mock("GET", "/user")
            .match_header("authorization", "Bearer token")
            .with_status(200)
            .with_body(r#"{"login":"may","name":"May"}"#)
            .create();
        let repository = server
            .mock("POST", "/user/repos")
            .match_header("authorization", "Bearer token")
            .match_body(Matcher::PartialJson(json!({"name":"skills-hub-sync","private":true})))
            .with_status(201)
            .with_body(r#"{"name":"skills-hub-sync","html_url":"https://example/repo","clone_url":"https://example/repo.git","ssh_url":"git@example:repo.git","private":true}"#)
            .create();
        let provider = ApiProvider::with_base_url(ProviderId::Github, server.url());
        assert_eq!(provider.validate_token("token").unwrap().login, "may");
        assert!(
            provider
                .create_private_repository("token", "skills-hub-sync")
                .unwrap()
                .private
        );
        account.assert();
        repository.assert();
    }

    #[test]
    fn provider_constructor_validates_the_application_proxy_for_every_provider() {
        for provider_id in [ProviderId::Github, ProviderId::Gitlab, ProviderId::Gitee] {
            assert!(provider(provider_id, "://invalid-proxy").is_err());
        }
    }

    #[test]
    fn every_provider_account_validation_uses_configured_proxy() {
        for provider_id in [ProviderId::Github, ProviderId::Gitlab, ProviderId::Gitee] {
            let (proxy_url, request, proxy) = account_proxy_once();
            let provider =
                ApiProvider::with_base_url_and_proxy(provider_id, "http://127.0.0.1:9", &proxy_url)
                    .unwrap();

            let account = provider.validate_token("token").unwrap();

            let received = request.recv_timeout(Duration::from_secs(5)).unwrap();
            proxy.join().unwrap();
            assert_eq!(account.login, "may");
            assert!(received.starts_with("GET http://127.0.0.1:9/user HTTP/1.1"));
        }
    }

    #[test]
    fn oauth_account_validation_does_not_follow_redirects() {
        for status in [307, 308] {
            let mut trusted_server = mockito::Server::new();
            let mut attacker_server = mockito::Server::new();
            let redirect_target = format!("{}/user", attacker_server.url());
            let trusted_request = trusted_server
                .mock("GET", "/user")
                .with_status(status)
                .with_header("location", &redirect_target)
                .create();
            let attacker_request = attacker_server
                .mock("GET", "/user")
                .expect(0)
                .with_status(200)
                .with_body(r#"{"login":"attacker"}"#)
                .create();
            let provider =
                ApiProvider::with_base_url_and_proxy(ProviderId::Github, trusted_server.url(), "")
                    .unwrap();

            let result = provider.validate_token("token");

            assert!(result.is_err(), "accepted HTTP {status} redirect");
            trusted_request.assert();
            attacker_request.assert();
        }
    }

    #[test]
    fn standard_provider_account_validation_preserves_redirect_behavior() {
        let mut trusted_server = mockito::Server::new();
        let mut destination_server = mockito::Server::new();
        let redirect_target = format!("{}/user", destination_server.url());
        let trusted_request = trusted_server
            .mock("GET", "/user")
            .with_status(307)
            .with_header("location", &redirect_target)
            .create();
        let destination_request = destination_server
            .mock("GET", "/user")
            .with_status(200)
            .with_body(r#"{"username":"gitlab-user"}"#)
            .create();
        let provider = ApiProvider::with_base_url(ProviderId::Gitlab, trusted_server.url());

        let account = provider.validate_token("token").unwrap();

        assert_eq!(account.login, "gitlab-user");
        trusted_request.assert();
        destination_request.assert();
    }

    #[test]
    fn gitlab_uses_bearer_token_and_normalizes_response() {
        let mut server = mockito::Server::new();
        let repository = server
            .mock("POST", "/projects")
            .match_header("authorization", "Bearer token")
            .with_status(201)
            .with_body(r#"{"name":"sync","web_url":"https://gitlab/repo","http_url_to_repo":"https://gitlab/repo.git","ssh_url_to_repo":"git@gitlab:repo.git","visibility":"private"}"#)
            .create();
        let provider = ApiProvider::with_base_url(ProviderId::Gitlab, server.url());
        let result = provider.create_private_repository("token", "sync").unwrap();
        assert_eq!(result.clone_url, "https://gitlab/repo.git");
        assert!(result.private);
        repository.assert();
    }

    #[test]
    fn lists_and_normalizes_repositories() {
        let mut server = mockito::Server::new();
        let repositories = server
            .mock(
                "GET",
                "/user/repos?visibility=all&affiliation=owner&per_page=100&sort=updated",
            )
            .match_header("authorization", "Bearer token")
            .with_status(200)
            .with_body(r#"[{"name":"sync","html_url":"https://example/repo","clone_url":"https://example/repo.git","private":true}]"#)
            .create();
        let provider = ApiProvider::with_base_url(ProviderId::Github, server.url());
        let result = provider.list_repositories("token").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].clone_url, "https://example/repo.git");
        repositories.assert();
    }

    #[test]
    fn provider_errors_do_not_include_multiline_payloads() {
        let mut server = mockito::Server::new();
        let request = server
            .mock("GET", "/user")
            .with_status(401)
            .with_body("bad\ncredential")
            .create();
        let provider = ApiProvider::with_base_url(ProviderId::Gitee, server.url());
        let error = provider.validate_token("token").unwrap_err().to_string();
        assert!(!error.contains('\n'));
        request.assert();
    }

    #[test]
    fn gitee_lists_repositories_with_supported_filters_and_real_response_shape() {
        let mut server = mockito::Server::new();
        let repositories = server
            .mock(
                "GET",
                "/user/repos?visibility=all&per_page=100&sort=updated",
            )
            .match_header("authorization", "Bearer token")
            .with_status(200)
            .with_body(r#"[{"name":"sync","html_url":"https://gitee/repo.git","clone_url":null,"ssh_url":"git@gitee:repo.git","private":true}]"#)
            .create();
        let provider = ApiProvider::with_base_url(ProviderId::Gitee, server.url());

        let result = provider.list_repositories("token").unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].clone_url, "https://gitee/repo.git");
        repositories.assert();
    }

    #[test]
    fn gitee_creates_private_repository() {
        let mut server = mockito::Server::new();
        let repository = server
            .mock("POST", "/user/repos")
            .match_header("authorization", "Bearer token")
            .with_status(201)
            .with_body(r#"{"name":"sync","html_url":"https://gitee/repo","clone_url":"https://gitee/repo.git","ssh_url":"git@gitee:repo.git","private":true}"#)
            .create();
        let provider = ApiProvider::with_base_url(ProviderId::Gitee, server.url());
        let result = provider.create_private_repository("token", "sync").unwrap();
        assert_eq!(result.clone_url, "https://gitee/repo.git");
        assert!(result.private);
        repository.assert();
    }
}
