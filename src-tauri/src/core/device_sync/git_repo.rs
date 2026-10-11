use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use git2::{
    build::RepoBuilder, Cred, IndexAddOption, Oid, RemoteCallbacks, RemoteRedirect, Repository,
    ResetType, Signature,
};

use super::types::{CredentialUsage, DeviceSyncConfig, DeviceSyncDevice};
use crate::core::network_proxy::{git_fetch_options, git_push_options};

pub fn open_or_clone(
    path: &Path,
    config: &DeviceSyncConfig,
    token: Option<&str>,
    proxy_url: &str,
) -> Result<Repository> {
    if path.join(".git").exists() {
        let repo = Repository::open(path).context("open device sync repository")?;
        let matches_origin = origin_matches(&repo, config);
        if matches_origin {
            Ok(repo)
        } else {
            drop(repo);
            std::fs::remove_dir_all(path).context("replace device sync workspace")?;
            open_or_clone(path, config, token, proxy_url)
        }
    } else {
        if path.exists() {
            std::fs::remove_dir_all(path).context("reset incomplete device sync workspace")?;
        }
        std::fs::create_dir_all(path.parent().context("sync workspace has no parent")?)?;
        let mut fetch = git_fetch_options(proxy_url);
        fetch.remote_callbacks(callbacks(config, token));
        fetch.follow_redirects(remote_redirect_policy(token));
        let mut builder = RepoBuilder::new();
        builder.fetch_options(fetch);
        builder
            .clone(&config.remote_url, path)
            .context("clone device sync repository")
    }
}

pub fn origin_matches(repo: &Repository, config: &DeviceSyncConfig) -> bool {
    repo.find_remote("origin")
        .ok()
        .and_then(|remote| remote.url().map(str::to_string))
        .as_deref()
        == Some(config.remote_url.as_str())
}

pub fn fetch_and_checkout(
    repo: &Repository,
    config: &DeviceSyncConfig,
    token: Option<&str>,
    proxy_url: &str,
) -> Result<Option<Oid>> {
    let mut remote = repo.find_remote("origin").context("find sync origin")?;
    let mut options = git_fetch_options(proxy_url);
    options.remote_callbacks(callbacks(config, token));
    options.follow_redirects(remote_redirect_policy(token));
    let refspec = format!(
        "refs/heads/{}:refs/remotes/origin/{}",
        config.branch, config.branch
    );
    remote
        .fetch(&[&refspec], Some(&mut options), None)
        .context("fetch device sync repository")?;
    let remote_ref = format!("refs/remotes/origin/{}", config.branch);
    let oid = match repo.refname_to_id(&remote_ref) {
        Ok(oid) => oid,
        Err(_) => return Ok(None),
    };
    let object = repo.find_object(oid, None)?;
    repo.reset(&object, ResetType::Hard, None)
        .context("checkout remote sync state")?;
    repo.set_head_detached(oid)?;
    Ok(Some(oid))
}

pub fn commit_all(repo: &Repository, message: &str, parent: Option<Oid>) -> Result<Option<Oid>> {
    commit_all_with_empty(repo, message, parent, false)
}

pub fn commit_all_allow_empty(
    repo: &Repository,
    message: &str,
    parent: Option<Oid>,
) -> Result<Option<Oid>> {
    commit_all_with_empty(repo, message, parent, true)
}

fn commit_all_with_empty(
    repo: &Repository,
    message: &str,
    parent: Option<Oid>,
    allow_empty: bool,
) -> Result<Option<Oid>> {
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    if let Some(parent_oid) = parent {
        let parent_commit = repo.find_commit(parent_oid)?;
        if !allow_empty && parent_commit.tree_id() == tree_oid {
            return Ok(None);
        }
        let signature = Signature::now("Skills Hub", "sync@skills-hub.local")?;
        let oid = repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent_commit],
        )?;
        Ok(Some(oid))
    } else {
        let signature = Signature::now("Skills Hub", "sync@skills-hub.local")?;
        let oid = repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])?;
        Ok(Some(oid))
    }
}

pub fn push(
    repo: &Repository,
    config: &DeviceSyncConfig,
    token: Option<&str>,
    oid: Oid,
    proxy_url: &str,
) -> Result<()> {
    let local_ref = format!("refs/heads/{}", config.branch);
    repo.reference(&local_ref, oid, true, "device sync")?;
    repo.set_head(&local_ref)?;
    let mut remote = repo.find_remote("origin")?;
    let mut options = git_push_options(proxy_url);
    let mut push_callbacks = callbacks(config, token);
    push_callbacks.push_update_reference(|_, status| {
        if status.is_some() {
            Err(git2::Error::from_str(
                "remote rejected device sync branch update",
            ))
        } else {
            Ok(())
        }
    });
    options.remote_callbacks(push_callbacks);
    options.follow_redirects(remote_redirect_policy(token));
    let refspec = format!("{}:{}", local_ref, local_ref);
    remote
        .push(&[&refspec], Some(&mut options))
        .context("push device sync repository")
}

pub fn manifest_at(repo: &Repository, oid: Oid) -> Result<super::manifest::SyncManifest> {
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    let entry = match tree.get_path(Path::new(super::manifest::MANIFEST_PATH)) {
        Ok(entry) => entry,
        Err(_) => return Ok(super::manifest::SyncManifest::empty()),
    };
    let blob = repo.find_blob(entry.id())?;
    super::manifest::SyncManifest::decode(blob.content())
}

pub fn skill_snapshot_at(
    repo: &Repository,
    oid: Oid,
    skill: &super::manifest::PortableSkill,
) -> Result<tempfile::TempDir> {
    use super::manifest::{hash_files, skill_dir, validate_relative_path};
    anyhow::ensure!(
        !skill.id.is_empty()
            && skill.id != "."
            && skill.id != ".."
            && !skill.id.contains(['/', '\\', ':']),
        "unsafe sync skill id"
    );
    let snapshot = tempfile::tempdir()?;
    let destination = skill_dir(snapshot.path(), &skill.id);
    std::fs::create_dir_all(&destination)?;
    let tree = repo.find_commit(oid)?.tree()?;
    for relative in skill.files.keys() {
        validate_relative_path(Path::new(relative))?;
        anyhow::ensure!(
            !relative.is_empty() && !relative.contains(['\\', ':']),
            "unsafe sync file path"
        );
        let entry = tree.get_path(
            &Path::new("skills")
                .join(&skill.id)
                .join("content")
                .join(relative),
        )?;
        anyhow::ensure!(
            matches!(entry.filemode(), 0o100644 | 0o100755),
            "unsafe sync file type"
        );
        let blob = repo.find_blob(entry.id())?;
        let target = destination.join(relative);
        std::fs::create_dir_all(target.parent().context("missing snapshot parent")?)?;
        std::fs::write(&target, blob.content())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                &target,
                std::fs::Permissions::from_mode(entry.filemode() as u32 & 0o777),
            )?;
        }
    }
    anyhow::ensure!(
        hash_files(&destination)? == skill.files,
        "sync snapshot content mismatch"
    );
    Ok(snapshot)
}

#[cfg(test)]
pub fn discover_devices(repo: &Repository) -> Result<Vec<DeviceSyncDevice>> {
    let head = match repo.head().ok().and_then(|head| head.target()) {
        Some(head) => head,
        None => return Ok(Vec::new()),
    };
    discover_devices_at(repo, head)
}

pub(super) fn device_identity(commit: &git2::Commit<'_>) -> Option<(String, String)> {
    let message = commit.message()?;
    let id = trailer_value(message, "Skills-Hub-Device-ID")?;
    let name = trailer_value(message, "Skills-Hub-Device-Name").unwrap_or(id);
    Some((id.to_string(), name.to_string()))
}

pub fn discover_devices_at(repo: &Repository, head: Oid) -> Result<Vec<DeviceSyncDevice>> {
    let mut walk = repo.revwalk().context("read device sync history")?;
    walk.push(head)?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
    let mut seen = HashSet::new();
    let mut devices = Vec::new();
    for oid in walk {
        let commit = repo.find_commit(oid?)?;
        let Some(message) = commit.message() else {
            continue;
        };
        let Some(id) = trailer_value(message, "Skills-Hub-Device-ID") else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            continue;
        }
        let name = trailer_value(message, "Skills-Hub-Device-Name").unwrap_or(id);
        devices.push(DeviceSyncDevice {
            id: id.to_string(),
            name: name.to_string(),
            alias: None,
            last_commit: Some(commit.id().to_string()),
            last_seen_at: commit.time().seconds().saturating_mul(1_000),
            is_current: false,
        });
    }
    Ok(devices)
}

pub fn latest_device_commit_at(
    repo: &Repository,
    head: Oid,
    device_id: &str,
) -> Result<Option<Oid>> {
    let mut walk = repo.revwalk().context("read device sync history")?;
    walk.push(head)?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
    for oid in walk {
        let commit = repo.find_commit(oid?)?;
        if commit
            .message()
            .and_then(|message| trailer_value(message, "Skills-Hub-Device-ID"))
            == Some(device_id)
        {
            return Ok(Some(commit.id()));
        }
    }
    Ok(None)
}

pub fn remote_head(repo: &Repository, config: &DeviceSyncConfig) -> Option<Oid> {
    repo.refname_to_id(&format!("refs/remotes/origin/{}", config.branch))
        .ok()
}

pub fn update_remote_head(repo: &Repository, config: &DeviceSyncConfig, oid: Oid) -> Result<()> {
    repo.reference(
        &format!("refs/remotes/origin/{}", config.branch),
        oid,
        true,
        "device sync push",
    )?;
    Ok(())
}

fn trailer_value<'a>(message: &'a str, key: &str) -> Option<&'a str> {
    message
        .lines()
        .rev()
        .take_while(|line| !line.trim().is_empty())
        .find_map(|line| {
            let (candidate, value) = line.split_once(':')?;
            (candidate.trim() == key)
                .then(|| value.trim())
                .filter(|value| !value.is_empty())
        })
}

fn callbacks<'a>(config: &'a DeviceSyncConfig, token: Option<&'a str>) -> RemoteCallbacks<'a> {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(move |url, username_from_url, allowed| {
        credential_for_callback(config, token, url, username_from_url, allowed)
    });
    callbacks
}

fn remote_redirect_policy(token: Option<&str>) -> RemoteRedirect {
    if token.is_some() {
        RemoteRedirect::None
    } else {
        RemoteRedirect::Initial
    }
}

fn credential_for_callback(
    config: &DeviceSyncConfig,
    token: Option<&str>,
    actual_url: &str,
    username_from_url: Option<&str>,
    allowed: git2::CredentialType,
) -> std::result::Result<Cred, git2::Error> {
    if allowed.is_ssh_key() {
        return Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"));
    }
    if let Some(token) = token {
        let expected = CredentialUsage::from_https_remote(config.provider, &config.remote_url)
            .map_err(|err| git2::Error::from_str(&err.to_string()))?;
        let actual = CredentialUsage::from_https_remote(config.provider, actual_url)
            .map_err(|err| git2::Error::from_str(&err.to_string()))?;
        if actual != expected {
            return Err(git2::Error::from_str(
                "refusing device sync credential for a different repository host",
            ));
        }
        let username = config
            .username
            .as_deref()
            .or(username_from_url)
            .unwrap_or("oauth2");
        return Cred::userpass_plaintext(username, token);
    }
    Cred::default()
}

#[cfg(test)]
fn remote_host(remote_url: &str) -> Option<&str> {
    let (_, remainder) = remote_url.split_once("://")?;
    let authority = remainder.split('/').next()?;
    authority
        .rsplit('@')
        .next()
        .and_then(|value| value.split(':').next())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Duration;

    fn commit_file(repo: &Repository, path: &str, content: &str, parent: Option<Oid>) -> Oid {
        let workdir = repo.workdir().unwrap();
        let file = workdir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
        commit_all(repo, "test", parent).unwrap().unwrap()
    }

    #[test]
    fn clone_commit_and_push_roundtrip_with_bare_repository() {
        let temp = tempfile::tempdir().unwrap();
        let bare_path = temp.path().join("remote.git");
        Repository::init_bare(&bare_path).unwrap();

        let seed_path = temp.path().join("seed");
        let seed = Repository::init(&seed_path).unwrap();
        let first = commit_file(
            &seed,
            ".skills-hub/manifest.json",
            r#"{"format_version":1,"skills":{}}"#,
            None,
        );
        seed.remote("origin", bare_path.to_str().unwrap()).unwrap();
        let config = DeviceSyncConfig {
            remote_url: bare_path.to_string_lossy().to_string(),
            ..DeviceSyncConfig::default()
        };
        push(&seed, &config, None, first, "").unwrap();

        let checkout_path = temp.path().join("checkout");
        let checkout = open_or_clone(&checkout_path, &config, None, "").unwrap();
        let parent = fetch_and_checkout(&checkout, &config, None, "")
            .unwrap()
            .unwrap();
        let second = commit_file(
            &checkout,
            "skills/one/content/SKILL.md",
            "# One",
            Some(parent),
        );
        push(&checkout, &config, None, second, "").unwrap();

        let remote = Repository::open_bare(&bare_path).unwrap();
        assert_eq!(remote.refname_to_id("refs/heads/main").unwrap(), second);
    }

    #[test]
    fn first_sync_initializes_an_empty_remote_repository() {
        let temp = tempfile::tempdir().unwrap();
        let bare_path = temp.path().join("remote.git");
        Repository::init_bare(&bare_path).unwrap();
        let config = DeviceSyncConfig {
            remote_url: bare_path.to_string_lossy().to_string(),
            ..DeviceSyncConfig::default()
        };

        let checkout = open_or_clone(&temp.path().join("checkout"), &config, None, "").unwrap();
        assert_eq!(
            fetch_and_checkout(&checkout, &config, None, "").unwrap(),
            None
        );
        let first = commit_file(
            &checkout,
            ".skills-hub/manifest.json",
            r#"{"format_version":1,"skills":{}}"#,
            None,
        );
        push(&checkout, &config, None, first, "").unwrap();

        let remote = Repository::open_bare(&bare_path).unwrap();
        assert_eq!(remote.refname_to_id("refs/heads/main").unwrap(), first);
    }

    #[test]
    fn stale_push_does_not_overwrite_remote_and_can_fetch_the_winning_commit() {
        let temp = tempfile::tempdir().unwrap();
        let bare = Repository::init_bare(temp.path().join("remote.git")).unwrap();
        let config = DeviceSyncConfig {
            remote_url: bare.path().to_string_lossy().to_string(),
            ..DeviceSyncConfig::default()
        };
        let seed = Repository::init(temp.path().join("seed")).unwrap();
        seed.remote("origin", &config.remote_url).unwrap();
        let base = commit_file(&seed, "shared.md", "base", None);
        push(&seed, &config, None, base, "").unwrap();
        let other = open_or_clone(&temp.path().join("other"), &config, None, "").unwrap();
        let stale_base = fetch_and_checkout(&other, &config, None, "").unwrap();
        let winning = commit_file(&seed, "shared.md", "remote edit", Some(base));
        push(&seed, &config, None, winning, "").unwrap();
        let losing = commit_file(&other, "shared.md", "local edit", stale_base);
        let error = push(&other, &config, None, losing, "").unwrap_err();
        assert_eq!(
            error.downcast_ref::<git2::Error>().unwrap().code(),
            git2::ErrorCode::NotFastForward
        );
        assert_eq!(bare.refname_to_id("refs/heads/main").unwrap(), winning);
        assert_eq!(
            fetch_and_checkout(&other, &config, None, "").unwrap(),
            Some(winning)
        );
        assert_eq!(
            fs::read_to_string(other.workdir().unwrap().join("shared.md")).unwrap(),
            "remote edit"
        );
    }

    #[test]
    fn clone_uses_the_explicit_application_proxy() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = mpsc::channel();
        let proxy = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            request_tx
                .send(String::from_utf8_lossy(&buffer[..read]).into_owned())
                .unwrap();
            stream
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        let config = DeviceSyncConfig {
            remote_url: "https://127.0.0.1:9/example/repo.git".to_string(),
            ..DeviceSyncConfig::default()
        };
        let destination = tempfile::tempdir().unwrap().path().join("clone");

        let result = open_or_clone(&destination, &config, None, &format!("http://{address}"));

        assert!(result.is_err());
        let request = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        proxy.join().unwrap();
        assert!(request.starts_with("CONNECT 127.0.0.1:9 HTTP/1.1"));
    }

    #[test]
    fn extracts_remote_host_without_credentials_or_port() {
        assert_eq!(
            remote_host("https://user@github.com:443/example/repo.git"),
            Some("github.com")
        );
    }

    #[test]
    fn credential_callback_rejects_a_different_actual_https_host() {
        let config = DeviceSyncConfig {
            provider: super::super::types::ProviderId::Github,
            remote_url: "https://github.com/example/sync.git".to_string(),
            ..DeviceSyncConfig::default()
        };

        let credential = credential_for_callback(
            &config,
            Some("secret-token"),
            "https://attacker.example/example/sync.git",
            None,
            git2::CredentialType::USER_PASS_PLAINTEXT,
        );

        assert!(credential.is_err());
    }

    #[test]
    fn credential_callback_uses_ssh_even_when_a_secret_is_present() {
        let config = DeviceSyncConfig {
            remote_url: "ssh://git@github.com/example/sync.git".to_string(),
            ..DeviceSyncConfig::default()
        };

        let credential = credential_for_callback(
            &config,
            Some("must-not-be-used-as-a-password"),
            "ssh://git@github.com/example/sync.git",
            Some("git"),
            git2::CredentialType::SSH_KEY,
        )
        .unwrap();

        assert_eq!(
            i64::from(credential.credtype()),
            i64::from(git2::CredentialType::SSH_KEY.bits())
        );
    }

    #[test]
    fn token_remote_operations_disable_offsite_redirects() {
        assert!(matches!(
            remote_redirect_policy(Some("secret-token")),
            git2::RemoteRedirect::None
        ));
    }

    #[test]
    fn discovers_latest_state_for_each_device_from_commit_trailers() {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let first = commit_file(
            &repo,
            ".skills-hub/manifest.json",
            r#"{"format_version":1,"skills":{}}"#,
            None,
        );
        let first_commit = repo.find_commit(first).unwrap();
        repo.commit(
            Some("HEAD"),
            &Signature::now("Skills Hub", "sync@skills-hub.local").unwrap(),
            &Signature::now("Skills Hub", "sync@skills-hub.local").unwrap(),
            "Sync Skills Hub library\n\nSkills-Hub-Device-ID: office-mac\nSkills-Hub-Device-Name: Office Mac",
            &first_commit.tree().unwrap(),
            &[&first_commit],
        )
        .unwrap();

        let devices = discover_devices(&repo).unwrap();

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "office-mac");
        assert_eq!(devices[0].name, "Office Mac");
    }

    #[test]
    fn device_registration_can_create_one_empty_metadata_commit() {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let parent = commit_file(&repo, "SKILL.md", "# Skill", None);

        let registration = commit_all_allow_empty(
            &repo,
            "Sync Skills Hub library\n\nSkills-Hub-Device-ID: new-device",
            Some(parent),
        )
        .unwrap()
        .unwrap();

        assert_ne!(registration, parent);
        assert_eq!(
            repo.find_commit(registration).unwrap().tree_id(),
            repo.find_commit(parent).unwrap().tree_id()
        );
    }

    #[test]
    fn ignores_device_fields_outside_the_trailer_footer() {
        assert_eq!(
            trailer_value(
                "Skills-Hub-Device-ID: fake\n\nThis is ordinary body text.",
                "Skills-Hub-Device-ID"
            ),
            None
        );
    }

    #[test]
    fn remote_head_ignores_an_unpushed_local_head_and_other_branches() {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let pushed = commit_file(&repo, "SKILL.md", "# One", None);
        let config = DeviceSyncConfig {
            remote_url: "https://example.com/sync.git".to_string(),
            ..DeviceSyncConfig::default()
        };
        repo.reference("refs/remotes/origin/main", pushed, true, "test")
            .unwrap();
        let unpushed = commit_file(&repo, "SKILL.md", "# Two", Some(pushed));

        assert_eq!(remote_head(&repo, &config), Some(pushed));
        assert_ne!(remote_head(&repo, &config), Some(unpushed));
        assert_eq!(
            remote_head(
                &repo,
                &DeviceSyncConfig {
                    branch: "another".to_string(),
                    ..config
                }
            ),
            None
        );
    }
}
