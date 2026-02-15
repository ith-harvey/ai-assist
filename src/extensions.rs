//! Extension manager — stub.

use crate::error::Error;

/// Extension manager for external tool integrations.
pub struct ExtensionManager;

impl ExtensionManager {
    /// Authenticate with an extension.
    pub async fn auth(
        &self,
        _extension_name: &str,
        _token: Option<&str>,
    ) -> Result<AuthResult, Error> {
        // No-op stub
        Ok(AuthResult {
            status: "authenticated".to_string(),
            instructions: None,
            auth_url: None,
            setup_url: None,
        })
    }

    /// Activate an extension.
    pub async fn activate(&self, _extension_name: &str) -> Result<ActivateResult, Error> {
        // No-op stub
        Ok(ActivateResult {
            tools_loaded: vec![],
        })
    }
}

/// Result of an authentication attempt.
#[derive(Debug)]
pub struct AuthResult {
    pub status: String,
    pub instructions: Option<String>,
    pub auth_url: Option<String>,
    pub setup_url: Option<String>,
}

/// Result of an activation attempt.
#[derive(Debug)]
pub struct ActivateResult {
    pub tools_loaded: Vec<String>,
}
