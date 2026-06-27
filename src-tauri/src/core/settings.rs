// Cognest Core — SettingsManager
// Encrypted config + macOS Keychain integration for API keys

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
use serde::{Deserialize, Serialize};

/// Keychain service name for the encryption key
const KEYCHAIN_SERVICE_SETTINGS: &str = "com.cognest.settings";
/// Keychain account name for the encryption key
const KEYCHAIN_ACCOUNT_ENCRYPTION_KEY: &str = "encryption-key";
/// Keychain service prefix for provider API keys
const KEYCHAIN_SERVICE_PROVIDER_PREFIX: &str = "com.cognest.provider.";

/// AES-256-GCM nonce size in bytes (96 bits)
const NONCE_SIZE: usize = 12;
/// AES-256 key size in bytes
const KEY_SIZE: usize = 32;

// ─── Types ──────────────────────────────────────────────────────────────────

/// Provider type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    DeepSeek,
    Ollama,
    OpenAiCompat,
}

/// Provider configuration (no plaintext API Key stored here)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub provider_type: ProviderType,
    pub endpoint: String,
    pub model: String,
    pub temperature: f32,
    pub enabled: bool,
}

/// Agent → Provider routing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRouting {
    pub default_provider: Option<String>,
    /// agent_name → provider_id
    pub overrides: HashMap<String, String>,
}

/// Complete application settings (serialized to .enc file, no API keys in plaintext)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub providers: Vec<ProviderConfig>,
    pub routing: AgentRouting,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            routing: AgentRouting {
                default_provider: None,
                overrides: HashMap::new(),
            },
        }
    }
}

/// Settings-related errors
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("配置文件不存在或不可读")]
    NotFound,
    #[error("解密失败")]
    DecryptionFailed,
    #[error("Keychain 访问失败: {0}")]
    KeychainError(String),
    #[error("序列化错误: {0}")]
    Serialization(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

// ─── SettingsManager ────────────────────────────────────────────────────────

/// Settings manager handling encrypted config file and macOS Keychain access
pub struct SettingsManager {
    /// Path to .cognest/settings.enc
    config_path: PathBuf,
}

impl SettingsManager {
    /// Create a new SettingsManager for the given vault path.
    /// The encrypted settings file will be stored at `<vault_path>/.cognest/settings.enc`.
    pub fn new(vault_path: &Path) -> Self {
        let config_path = vault_path.join(".cognest").join("settings.enc");
        Self { config_path }
    }

    /// Get the directory path for storing pinned views.
    /// Returns `<vault_path>/.cognest/views/`.
    pub fn views_dir(&self) -> PathBuf {
        self.config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("views")
    }

    /// Load settings from the encrypted config file.
    /// Returns default empty settings if the file doesn't exist or decryption fails.
    pub fn load(&self) -> Result<AppSettings, SettingsError> {
        if !self.config_path.exists() {
            return Ok(AppSettings::default());
        }

        let ciphertext = fs::read(&self.config_path).map_err(|_| SettingsError::NotFound)?;

        // Need at least nonce + some ciphertext
        if ciphertext.len() <= NONCE_SIZE {
            return Ok(AppSettings::default());
        }

        let encryption_key = match self.get_or_create_encryption_key() {
            Ok(key) => key,
            Err(_) => return Ok(AppSettings::default()),
        };

        let nonce_bytes = &ciphertext[..NONCE_SIZE];
        let encrypted_data = &ciphertext[NONCE_SIZE..];

        let cipher = Aes256Gcm::new_from_slice(&encryption_key)
            .map_err(|_| SettingsError::DecryptionFailed)?;
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = match cipher.decrypt(nonce, encrypted_data) {
            Ok(data) => data,
            Err(_) => return Ok(AppSettings::default()),
        };

        let settings: AppSettings = serde_json::from_slice(&plaintext)
            .map_err(|e| SettingsError::Serialization(e.to_string()))?;

        Ok(settings)
    }

    /// Save settings to the encrypted config file using AES-256-GCM.
    pub fn save(&self, settings: &AppSettings) -> Result<(), SettingsError> {
        let encryption_key = self.get_or_create_encryption_key()?;

        let plaintext =
            serde_json::to_vec(settings).map_err(|e| SettingsError::Serialization(e.to_string()))?;

        let cipher = Aes256Gcm::new_from_slice(&encryption_key)
            .map_err(|_| SettingsError::DecryptionFailed)?;

        // Generate a random nonce for each save
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|_| SettingsError::DecryptionFailed)?;

        // Ensure parent directory exists
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write nonce + ciphertext
        let mut output = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        fs::write(&self.config_path, &output)?;

        Ok(())
    }

    /// Retrieve an API key from macOS Keychain for a specific provider.
    /// Returns Ok(None) if the key doesn't exist in Keychain.
    pub fn get_api_key(&self, provider_id: &str) -> Result<Option<String>, SettingsError> {
        let service = format!("{}{}", KEYCHAIN_SERVICE_PROVIDER_PREFIX, provider_id);

        match get_generic_password(&service, provider_id) {
            Ok(bytes) => {
                let key = String::from_utf8(bytes.to_vec()).map_err(|e| {
                    SettingsError::KeychainError(format!("Invalid UTF-8 in keychain: {}", e))
                })?;
                Ok(Some(key))
            }
            Err(e) => {
                // errSecItemNotFound (-25300) → key doesn't exist, return None
                let err_str = e.to_string();
                if err_str.contains("found") || err_str.contains("-25300") {
                    Ok(None)
                } else {
                    Err(SettingsError::KeychainError(err_str))
                }
            }
        }
    }

    /// Store an API key in macOS Keychain for a specific provider.
    pub fn set_api_key(&self, provider_id: &str, key: &str) -> Result<(), SettingsError> {
        let service = format!("{}{}", KEYCHAIN_SERVICE_PROVIDER_PREFIX, provider_id);

        // Delete existing key first (if any) to avoid duplicate item errors
        let _ = delete_generic_password(&service, provider_id);

        set_generic_password(&service, provider_id, key.as_bytes())
            .map_err(|e| SettingsError::KeychainError(e.to_string()))?;

        Ok(())
    }

    /// Delete an API key from macOS Keychain for a specific provider.
    pub fn delete_api_key(&self, provider_id: &str) -> Result<(), SettingsError> {
        let service = format!("{}{}", KEYCHAIN_SERVICE_PROVIDER_PREFIX, provider_id);

        match delete_generic_password(&service, provider_id) {
            Ok(()) => Ok(()),
            Err(e) => {
                let err_str = e.to_string();
                // If already not found, treat as success
                if err_str.contains("found") || err_str.contains("-25300") {
                    Ok(())
                } else {
                    Err(SettingsError::KeychainError(err_str))
                }
            }
        }
    }

    // ─── Private Helpers ────────────────────────────────────────────────────

    /// Get or create the AES-256 encryption key stored in Keychain.
    /// If no key exists, generates a random 256-bit key and stores it.
    fn get_or_create_encryption_key(&self) -> Result<Vec<u8>, SettingsError> {
        match get_generic_password(KEYCHAIN_SERVICE_SETTINGS, KEYCHAIN_ACCOUNT_ENCRYPTION_KEY) {
            Ok(key_bytes) => {
                if key_bytes.len() == KEY_SIZE {
                    Ok(key_bytes.to_vec())
                } else {
                    // Key has wrong size, regenerate
                    let new_key = self.generate_and_store_encryption_key()?;
                    Ok(new_key)
                }
            }
            Err(_) => {
                // Key doesn't exist, create one
                let new_key = self.generate_and_store_encryption_key()?;
                Ok(new_key)
            }
        }
    }

    /// Generate a random 256-bit key and store it in Keychain.
    fn generate_and_store_encryption_key(&self) -> Result<Vec<u8>, SettingsError> {
        let mut key = vec![0u8; KEY_SIZE];
        OsRng.fill_bytes(&mut key);

        // Delete old key if exists
        let _ = delete_generic_password(KEYCHAIN_SERVICE_SETTINGS, KEYCHAIN_ACCOUNT_ENCRYPTION_KEY);

        set_generic_password(
            KEYCHAIN_SERVICE_SETTINGS,
            KEYCHAIN_ACCOUNT_ENCRYPTION_KEY,
            &key,
        )
        .map_err(|e| SettingsError::KeychainError(e.to_string()))?;

        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_settings_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        let manager = SettingsManager::new(tmp.path());

        let settings = manager.load().unwrap();
        assert!(settings.providers.is_empty());
        assert!(settings.routing.default_provider.is_none());
        assert!(settings.routing.overrides.is_empty());
    }

    #[test]
    fn test_save_and_load_round_trip() {
        let tmp = TempDir::new().unwrap();
        let manager = SettingsManager::new(tmp.path());

        let settings = AppSettings {
            providers: vec![ProviderConfig {
                id: "deepseek-1".to_string(),
                name: "DeepSeek".to_string(),
                provider_type: ProviderType::DeepSeek,
                endpoint: "https://api.deepseek.com".to_string(),
                model: "deepseek-chat".to_string(),
                temperature: 0.7,
                enabled: true,
            }],
            routing: AgentRouting {
                default_provider: Some("deepseek-1".to_string()),
                overrides: HashMap::from([
                    ("curator".to_string(), "ollama-1".to_string()),
                ]),
            },
        };

        manager.save(&settings).unwrap();

        let loaded = manager.load().unwrap();
        assert_eq!(loaded.providers.len(), 1);
        assert_eq!(loaded.providers[0].id, "deepseek-1");
        assert_eq!(loaded.providers[0].name, "DeepSeek");
        assert_eq!(loaded.providers[0].provider_type, ProviderType::DeepSeek);
        assert_eq!(loaded.providers[0].endpoint, "https://api.deepseek.com");
        assert_eq!(loaded.providers[0].model, "deepseek-chat");
        assert!((loaded.providers[0].temperature - 0.7).abs() < f32::EPSILON);
        assert!(loaded.providers[0].enabled);
        assert_eq!(
            loaded.routing.default_provider,
            Some("deepseek-1".to_string())
        );
        assert_eq!(
            loaded.routing.overrides.get("curator"),
            Some(&"ollama-1".to_string())
        );
    }

    #[test]
    fn test_encrypted_file_not_plaintext() {
        let tmp = TempDir::new().unwrap();
        let manager = SettingsManager::new(tmp.path());

        let settings = AppSettings {
            providers: vec![ProviderConfig {
                id: "test-provider".to_string(),
                name: "Test".to_string(),
                provider_type: ProviderType::Ollama,
                endpoint: "http://localhost:11434".to_string(),
                model: "qwen2.5".to_string(),
                temperature: 0.5,
                enabled: true,
            }],
            routing: AgentRouting {
                default_provider: None,
                overrides: HashMap::new(),
            },
        };

        manager.save(&settings).unwrap();

        // Read raw file bytes — should NOT contain plaintext provider name
        let raw = fs::read(tmp.path().join(".cognest").join("settings.enc")).unwrap();
        let raw_str = String::from_utf8_lossy(&raw);
        assert!(
            !raw_str.contains("test-provider"),
            "Encrypted file should not contain plaintext"
        );
    }

    #[test]
    fn test_corrupted_file_returns_default() {
        let tmp = TempDir::new().unwrap();
        let enc_dir = tmp.path().join(".cognest");
        fs::create_dir_all(&enc_dir).unwrap();
        fs::write(enc_dir.join("settings.enc"), b"corrupted data here").unwrap();

        let manager = SettingsManager::new(tmp.path());
        let settings = manager.load().unwrap();
        assert!(settings.providers.is_empty());
    }

    /// This test requires macOS Keychain access and may prompt for permission.
    /// Run manually with: cargo test --lib core::settings::tests::test_keychain_api_key_crud -- --ignored
    #[test]
    #[ignore = "Requires interactive macOS Keychain access"]
    fn test_keychain_api_key_crud() {
        let tmp = TempDir::new().unwrap();
        let manager = SettingsManager::new(tmp.path());
        let provider_id = "test-keychain-crud";

        // Clean up first in case of previous test failures
        let _ = manager.delete_api_key(provider_id);

        // Initially should be None
        let key = manager.get_api_key(provider_id).unwrap();
        assert_eq!(key, None);

        // Set a key
        manager.set_api_key(provider_id, "sk-test-12345").unwrap();

        // Should retrieve it
        let key = manager.get_api_key(provider_id).unwrap();
        assert_eq!(key, Some("sk-test-12345".to_string()));

        // Update the key
        manager.set_api_key(provider_id, "sk-updated-67890").unwrap();
        let key = manager.get_api_key(provider_id).unwrap();
        assert_eq!(key, Some("sk-updated-67890".to_string()));

        // Delete it
        manager.delete_api_key(provider_id).unwrap();
        let key = manager.get_api_key(provider_id).unwrap();
        assert_eq!(key, None);

        // Deleting again should succeed (idempotent)
        manager.delete_api_key(provider_id).unwrap();
    }
}
