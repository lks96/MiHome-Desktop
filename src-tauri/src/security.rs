use crate::errors::AppError;
use zeroize::Zeroizing;

pub trait CredentialStore: Send + Sync {
    fn read(&self, reference: &str) -> Result<Zeroizing<String>, AppError>;
    fn read_optional(&self, reference: &str) -> Result<Option<Zeroizing<String>>, AppError> {
        self.read(reference).map(Some)
    }
    fn write(&self, reference: &str, token: &str) -> Result<(), AppError>;
    fn delete(&self, reference: &str) -> Result<(), AppError>;
}

pub struct SystemCredentials;
fn credential_error() -> AppError {
    AppError::new(
        "CREDENTIAL_STORE_FAILED",
        "无法访问 Windows 凭据管理器，请检查系统凭据服务",
        false,
    )
}
#[cfg(windows)]
impl CredentialStore for SystemCredentials {
    fn read_optional(&self, reference: &str) -> Result<Option<Zeroizing<String>>, AppError> {
        match keyring::Entry::new("com.lks96.mihome-desktop", reference)
            .and_then(|entry| entry.get_password())
        {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(credential_error()),
        }
    }
    fn read(&self, reference: &str) -> Result<Zeroizing<String>, AppError> {
        keyring::Entry::new("com.lks96.mihome-desktop", reference)
            .and_then(|entry| entry.get_password())
            .map(Zeroizing::new)
            .map_err(|_| credential_error())
    }
    fn write(&self, reference: &str, token: &str) -> Result<(), AppError> {
        keyring::Entry::new("com.lks96.mihome-desktop", reference)
            .and_then(|entry| entry.set_password(token))
            .map_err(|_| credential_error())
    }
    fn delete(&self, reference: &str) -> Result<(), AppError> {
        match keyring::Entry::new("com.lks96.mihome-desktop", reference)
            .and_then(|entry| entry.delete_credential())
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(credential_error()),
        }
    }
}
#[cfg(not(windows))]
impl CredentialStore for SystemCredentials {
    fn read(&self, _: &str) -> Result<Zeroizing<String>, AppError> {
        Err(credential_error())
    }
    fn write(&self, _: &str, _: &str) -> Result<(), AppError> {
        Err(credential_error())
    }
    fn delete(&self, _: &str) -> Result<(), AppError> {
        Err(credential_error())
    }
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct MemoryCredentials(
    pub std::sync::Mutex<std::collections::HashMap<String, String>>,
);
#[cfg(test)]
impl CredentialStore for MemoryCredentials {
    fn read_optional(&self, reference: &str) -> Result<Option<Zeroizing<String>>, AppError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(reference)
            .cloned()
            .map(Zeroizing::new))
    }
    fn read(&self, reference: &str) -> Result<Zeroizing<String>, AppError> {
        self.0
            .lock()
            .unwrap()
            .get(reference)
            .cloned()
            .map(Zeroizing::new)
            .ok_or_else(credential_error)
    }
    fn write(&self, reference: &str, token: &str) -> Result<(), AppError> {
        self.0
            .lock()
            .unwrap()
            .insert(reference.into(), token.into());
        Ok(())
    }
    fn delete(&self, reference: &str) -> Result<(), AppError> {
        self.0.lock().unwrap().remove(reference);
        Ok(())
    }
}
