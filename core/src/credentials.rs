//! Where the Garmin account's credentials live: the email address in
//! `~/.config/nimrag-sync/account`, the password in the desktop's keyring
//! (the Secret Service, through libsecret's `secret-tool`), never in a file.
//! `$NIMRAG_NETRC` names a netrc file that overrides both, for scripts and
//! tests.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::server::{self, Credentials};

const SERVICE: &str = "nimrag-sync";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not signed in: run `nimrag-sync login <email>` or sign in in the window")]
    NotSignedIn,
    #[error("the keyring has no password for {0}: sign in again")]
    NoPassword(String),
    #[error("cannot use the keyring ({0}); is a Secret Service such as gnome-keyring running?")]
    Keyring(String),
    #[error("{0}")]
    File(String),
}

/// The password store: the keyring through `secret-tool` (tests use a stand-in).
pub struct Keyring {
    secret_tool: PathBuf,
    /// Arguments before secret-tool's own: the tests run their stand-in as
    /// `/bin/sh <script>`, since executing a file just written can fail with
    /// "Text file busy" while another thread forks.
    prefix: Vec<PathBuf>,
}

impl Default for Keyring {
    fn default() -> Self {
        Keyring { secret_tool: PathBuf::from("secret-tool"), prefix: Vec::new() }
    }
}

impl Keyring {
    pub fn with_tool(path: &Path) -> Keyring {
        Keyring { secret_tool: path.to_path_buf(), prefix: Vec::new() }
    }

    fn run(&self, args: &[&str], input: Option<&str>) -> Result<(bool, String), Error> {
        let mut child = Command::new(&self.secret_tool)
            .args(&self.prefix)
            .args(args)
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Keyring(format!("secret-tool: {e}")))?;
        if let Some(text) = input {
            // secret-tool reads the secret from a pipe as it is, without a newline.
            let mut stdin = child.stdin.take().expect("piped");
            stdin.write_all(text.as_bytes()).map_err(|e| Error::Keyring(e.to_string()))?;
        }
        let out = child.wait_with_output().map_err(|e| Error::Keyring(e.to_string()))?;
        if !out.status.success() && !out.stderr.is_empty() {
            return Err(Error::Keyring(String::from_utf8_lossy(&out.stderr).trim().to_string()));
        }
        Ok((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned()))
    }

    pub fn store(&self, email: &str, password: &str) -> Result<(), Error> {
        let label = format!("nimrag-sync: Garmin account {email}");
        let (ok, _) = self.run(&["store", "--label", &label, "service", SERVICE, "account", email], Some(password))?;
        if ok { Ok(()) } else { Err(Error::Keyring("secret-tool store failed".into())) }
    }

    /// The password, or `None` when the keyring has none for `email`.
    pub fn lookup(&self, email: &str) -> Result<Option<String>, Error> {
        let (ok, out) = self.run(&["lookup", "service", SERVICE, "account", email], None)?;
        Ok(if ok && !out.is_empty() { Some(out) } else { None })
    }

    pub fn clear(&self, email: &str) -> Result<(), Error> {
        self.run(&["clear", "service", SERVICE, "account", email], None).map(|_| ())
    }
}

/// `$XDG_CONFIG_HOME/nimrag-sync`, or `~/.config/nimrag-sync`.
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("nimrag-sync")
}

/// The signed-in account's email address, kept in `dir/account`.
pub fn signed_in_email(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("account")).ok()?;
    let email = text.trim();
    (!email.is_empty()).then(|| email.to_string())
}

/// Signs in: the password goes to the keyring, then the email address to
/// `dir/account` (written beside it and renamed, so it is never half written).
pub fn sign_in(dir: &Path, keyring: &Keyring, email: &str, password: &str) -> Result<(), Error> {
    let email = email.trim();
    if email.is_empty() || !email.contains('@') {
        return Err(Error::File(format!("{email:?} is not an email address")));
    }
    if password.is_empty() {
        return Err(Error::File("the password is empty".into()));
    }
    keyring.store(email, password)?;
    std::fs::create_dir_all(dir).map_err(|e| Error::File(format!("{}: {e}", dir.display())))?;
    let path = dir.join("account");
    let partial = dir.join("account.partial");
    std::fs::write(&partial, format!("{email}\n"))
        .and_then(|_| std::fs::rename(&partial, &path))
        .map_err(|e| Error::File(format!("{}: {e}", path.display())))
}

/// Signs out: the password leaves the keyring and the email address its file.
pub fn sign_out(dir: &Path, keyring: &Keyring) -> Result<Option<String>, Error> {
    let Some(email) = signed_in_email(dir) else { return Ok(None) };
    keyring.clear(&email)?;
    let _ = std::fs::remove_file(dir.join("account"));
    Ok(Some(email))
}

/// The credentials for `host`: from `$NIMRAG_NETRC` when it is set, else the
/// signed-in account and its password in the keyring.
pub fn load(dir: &Path, keyring: &Keyring, host: &str) -> Result<Credentials, Error> {
    if let Some(path) = std::env::var_os("NIMRAG_NETRC").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(path);
        let text = std::fs::read_to_string(&path).map_err(|e| Error::File(format!("{}: {e}", path.display())))?;
        return server::netrc_credentials(&text, host)
            .ok_or_else(|| Error::File(format!("{} has no login and password for {host}", path.display())));
    }
    let email = signed_in_email(dir).ok_or(Error::NotSignedIn)?;
    let password = keyring.lookup(&email)?.ok_or_else(|| Error::NoPassword(email.clone()))?;
    Ok(Credentials { email, password })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A secret-tool stand-in keeping secrets as files in `dir/secrets`.
    fn fake_tool(dir: &Path) -> Keyring {
        let script = dir.join("secret-tool");
        let store = dir.join("secrets");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(
            &script,
            format!(
                r#"#!/bin/sh
# The attributes are the last four arguments: service <s> account <a>.
eval "account=\${{$#}}"
f="{}/$account"
case "$1" in
  store) cat > "$f" ;;
  lookup) [ -f "$f" ] && cat "$f" || exit 1 ;;
  clear) rm -f "$f" ;;
esac
"#,
                store.display()
            ),
        )
        .unwrap();
        Keyring { secret_tool: PathBuf::from("/bin/sh"), prefix: vec![script] }
    }

    #[test]
    fn sign_in_load_and_sign_out() {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, keyring) = (tmp.path().join("config"), fake_tool(tmp.path()));
        assert!(matches!(load(&dir, &keyring, "h"), Err(Error::NotSignedIn)));
        sign_in(&dir, &keyring, " ada@example.com ", "pa ss\nword").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("account")).unwrap(), "ada@example.com\n");
        // The password is in the keyring, never in the config folder.
        for entry in std::fs::read_dir(&dir).unwrap() {
            assert!(!std::fs::read_to_string(entry.unwrap().path()).unwrap().contains("pa ss"));
        }
        let c = load(&dir, &keyring, "h").unwrap();
        assert_eq!((c.email.as_str(), c.password.as_str()), ("ada@example.com", "pa ss\nword"));
        assert_eq!(sign_out(&dir, &keyring).unwrap().as_deref(), Some("ada@example.com"));
        assert!(keyring.lookup("ada@example.com").unwrap().is_none());
        assert!(matches!(load(&dir, &keyring, "h"), Err(Error::NotSignedIn)));
        assert_eq!(sign_out(&dir, &keyring).unwrap(), None);
    }

    #[test]
    fn a_missing_password_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, keyring) = (tmp.path().join("config"), fake_tool(tmp.path()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("account"), "ada@example.com\n").unwrap();
        assert!(matches!(load(&dir, &keyring, "h"), Err(Error::NoPassword(_))));
    }

    #[test]
    fn sign_in_wants_an_email_and_a_password() {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, keyring) = (tmp.path().join("config"), fake_tool(tmp.path()));
        assert!(sign_in(&dir, &keyring, "ada", "x").is_err());
        assert!(sign_in(&dir, &keyring, "ada@example.com", "").is_err());
        assert!(signed_in_email(&dir).is_none());
    }

    #[test]
    fn a_missing_secret_tool_is_reported() {
        let keyring = Keyring::with_tool(Path::new("/nonexistent/secret-tool"));
        assert!(matches!(keyring.lookup("a@b"), Err(Error::Keyring(_))));
    }
}
