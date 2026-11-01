//! Where the GitHub sign-in is kept: the Keychain for the app, memory for its tests.

use crate::Result;

/// Where the sign-in is kept. The app keeps it in the Keychain ([`Keychain::user`]); its tests
/// keep theirs in memory ([`MemoryStore`]), so no test signs the user in or out.
pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<String>>;
    fn save(&self, token: &str) -> Result<()>;
    fn delete(&self) -> Result<()>;
}

/// A Keychain item holding the token: the one the app signs in with, or a test's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keychain {
    service: String,
    account: String,
}

impl Keychain {
    /// The user's sign-in: every GitHub control in the app works with it. A Sawit run with a
    /// data folder of its own (`SAWIT_HOME`: a scratch copy, the demo data the README's
    /// screenshots are taken with) keeps a sign-in of its own too, so it neither shows nor
    /// signs out the user's GitHub account.
    pub fn user() -> Self {
        Keychain {
            service: "sawit".into(),
            account: user_account(std::env::var_os("SAWIT_HOME").as_deref()),
        }
    }

    /// An item of a test's own, under another service, away from the user's.
    pub fn named(service: &str, account: &str) -> Self {
        Keychain {
            service: service.into(),
            account: account.into(),
        }
    }
}

/// The Keychain account of the GitHub sign-in: `github`, or with a data folder of its own
/// (`SAWIT_HOME`) `github:<that folder>`.
pub(super) fn user_account(home: Option<&std::ffi::OsStr>) -> String {
    match home.filter(|h| !h.is_empty()) {
        Some(home) => format!("github:{}", home.to_string_lossy()),
        None => "github".into(),
    }
}

impl TokenStore for Keychain {
    fn load(&self) -> Result<Option<String>> {
        crate::keychain::get(&self.service, &self.account)
    }

    fn save(&self, token: &str) -> Result<()> {
        crate::keychain::set(&self.service, &self.account, token)
    }

    fn delete(&self) -> Result<()> {
        crate::keychain::delete(&self.service, &self.account)
    }
}

/// A token kept in memory only, for tests.
#[derive(Default)]
pub struct MemoryStore(std::sync::Mutex<Option<String>>);

impl MemoryStore {
    pub fn with(token: &str) -> Self {
        MemoryStore(std::sync::Mutex::new(Some(token.to_string())))
    }
}

impl TokenStore for MemoryStore {
    fn load(&self) -> Result<Option<String>> {
        Ok(self.0.lock().map(|t| t.clone()).unwrap_or_default())
    }

    fn save(&self, token: &str) -> Result<()> {
        if let Ok(mut t) = self.0.lock() {
            *t = Some(token.to_string());
        }
        Ok(())
    }

    fn delete(&self) -> Result<()> {
        if let Ok(mut t) = self.0.lock() {
            *t = None;
        }
        Ok(())
    }
}
