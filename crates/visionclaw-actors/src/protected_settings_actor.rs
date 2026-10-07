use actix::prelude::*;
use log::info;
use serde_json::Value;

use visionclaw_domain::models::protected_settings::{ApiKeys, NostrUser, ProtectedSettings};

pub struct ProtectedSettingsActor {
    settings: ProtectedSettings,
}

impl ProtectedSettingsActor {
    pub fn new(settings: ProtectedSettings) -> Self {
        info!("ProtectedSettingsActor created");
        Self { settings }
    }
}

impl Actor for ProtectedSettingsActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("ProtectedSettingsActor started");
    }
}

// Message to get API keys for a user
#[derive(Message)]
#[rtype(result = "ApiKeys")]
pub struct GetApiKeys {
    pub pubkey: String,
}

impl Handler<GetApiKeys> for ProtectedSettingsActor {
    type Result = MessageResult<GetApiKeys>;

    fn handle(&mut self, msg: GetApiKeys, _ctx: &mut Self::Context) -> Self::Result {
        MessageResult(self.settings.get_api_keys(&msg.pubkey))
    }
}

// Message to validate client token
#[derive(Message)]
#[rtype(result = "bool")]
pub struct ValidateClientToken {
    pub pubkey: String,
    pub token: String,
}

impl Handler<ValidateClientToken> for ProtectedSettingsActor {
    type Result = bool;

    fn handle(&mut self, msg: ValidateClientToken, _ctx: &mut Self::Context) -> Self::Result {
        self.settings.validate_client_token(&msg.pubkey, &msg.token)
    }
}

// Message to store client token
#[derive(Message)]
#[rtype(result = "()")]
pub struct StoreClientToken {
    pub pubkey: String,
    pub token: String,
}

impl Handler<StoreClientToken> for ProtectedSettingsActor {
    type Result = ();

    fn handle(&mut self, msg: StoreClientToken, _ctx: &mut Self::Context) -> Self::Result {
        self.settings.store_client_token(msg.pubkey, msg.token);
    }
}

// Message to update user API keys
#[derive(Message)]
#[rtype(result = "Result<NostrUser, String>")]
pub struct UpdateUserApiKeys {
    pub pubkey: String,
    pub api_keys: ApiKeys,
}

impl Handler<UpdateUserApiKeys> for ProtectedSettingsActor {
    type Result = Result<NostrUser, String>;

    fn handle(&mut self, msg: UpdateUserApiKeys, _ctx: &mut Self::Context) -> Self::Result {
        self.settings
            .update_user_api_keys(&msg.pubkey, msg.api_keys)
    }
}

// Message to cleanup expired tokens
#[derive(Message)]
#[rtype(result = "()")]
pub struct CleanupExpiredTokens {
    pub max_age_hours: i64,
}

impl Handler<CleanupExpiredTokens> for ProtectedSettingsActor {
    type Result = ();

    fn handle(&mut self, msg: CleanupExpiredTokens, _ctx: &mut Self::Context) -> Self::Result {
        self.settings.cleanup_expired_tokens(msg.max_age_hours);
    }
}

// Message to merge settings
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct MergeSettings {
    pub settings: Value,
}

impl Handler<MergeSettings> for ProtectedSettingsActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: MergeSettings, _ctx: &mut Self::Context) -> Self::Result {
        self.settings.merge(msg.settings)
    }
}

// Message to save settings to file
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct SaveSettings {
    pub path: String,
}

impl Handler<SaveSettings> for ProtectedSettingsActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SaveSettings, _ctx: &mut Self::Context) -> Self::Result {
        self.settings.save(&msg.path)
    }
}

// Message to get user by pubkey
#[derive(Message)]
#[rtype(result = "Option<NostrUser>")]
pub struct GetUser {
    pub pubkey: String,
}

impl Handler<GetUser> for ProtectedSettingsActor {
    type Result = Option<NostrUser>;

    fn handle(&mut self, msg: GetUser, _ctx: &mut Self::Context) -> Self::Result {
        self.settings.users.get(&msg.pubkey).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use visionclaw_domain::models::protected_settings::{ApiKeys, NostrUser, ProtectedSettings};

    fn settings_with_user(pubkey: &str, is_power_user: bool) -> ProtectedSettings {
        let mut s = ProtectedSettings::default();
        s.users.insert(
            pubkey.to_string(),
            NostrUser {
                pubkey: pubkey.to_string(),
                npub: "npub1test".to_string(),
                is_power_user,
                api_keys: ApiKeys {
                    perplexity: Some("pplx-key".to_string()),
                    openai: Some("openai-key".to_string()),
                    ragflow: None,
                },
                last_seen: 0,
                session_token: None,
            },
        );
        s
    }

    #[actix::test]
    async fn get_api_keys_returns_user_keys_for_regular_user() {
        let settings = settings_with_user("pubkey1", false);
        let actor = ProtectedSettingsActor::new(settings).start();

        let keys = actor
            .send(GetApiKeys {
                pubkey: "pubkey1".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(keys.openai.as_deref(), Some("openai-key"));
        assert_eq!(keys.perplexity.as_deref(), Some("pplx-key"));
    }

    #[actix::test]
    async fn get_api_keys_returns_default_for_unknown_user() {
        let settings = ProtectedSettings {
            default_api_keys: ApiKeys {
                perplexity: None,
                openai: Some("default-key".to_string()),
                ragflow: None,
            },
            ..Default::default()
        };
        let actor = ProtectedSettingsActor::new(settings).start();

        let keys = actor
            .send(GetApiKeys {
                pubkey: "nobody".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(keys.openai.as_deref(), Some("default-key"));
    }

    #[actix::test]
    async fn validate_token_true_when_correct() {
        let mut settings = settings_with_user("pk2", false);
        settings.users.get_mut("pk2").unwrap().session_token = Some("tok123".to_string());
        let actor = ProtectedSettingsActor::new(settings).start();

        let valid = actor
            .send(ValidateClientToken {
                pubkey: "pk2".to_string(),
                token: "tok123".to_string(),
            })
            .await
            .unwrap();
        assert!(valid);
    }

    #[actix::test]
    async fn validate_token_false_for_wrong_token() {
        let mut settings = settings_with_user("pk3", false);
        settings.users.get_mut("pk3").unwrap().session_token = Some("right".to_string());
        let actor = ProtectedSettingsActor::new(settings).start();

        let valid = actor
            .send(ValidateClientToken {
                pubkey: "pk3".to_string(),
                token: "wrong".to_string(),
            })
            .await
            .unwrap();
        assert!(!valid);
    }

    #[actix::test]
    async fn store_then_validate_token() {
        let settings = settings_with_user("pk4", false);
        let actor = ProtectedSettingsActor::new(settings).start();

        actor
            .send(StoreClientToken {
                pubkey: "pk4".to_string(),
                token: "newtoken".to_string(),
            })
            .await
            .unwrap();

        let valid = actor
            .send(ValidateClientToken {
                pubkey: "pk4".to_string(),
                token: "newtoken".to_string(),
            })
            .await
            .unwrap();
        assert!(valid);
    }

    #[actix::test]
    async fn update_user_api_keys_persists_in_actor_state() {
        let settings = settings_with_user("pk5", false);
        let actor = ProtectedSettingsActor::new(settings).start();

        let new_keys = ApiKeys {
            perplexity: Some("new-pplx".to_string()),
            openai: None,
            ragflow: None,
        };
        let result = actor
            .send(UpdateUserApiKeys {
                pubkey: "pk5".to_string(),
                api_keys: new_keys,
            })
            .await
            .unwrap();
        assert!(result.is_ok());

        let fetched = actor
            .send(GetApiKeys {
                pubkey: "pk5".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(fetched.perplexity.as_deref(), Some("new-pplx"));
        assert!(fetched.openai.is_none());
    }

    #[actix::test]
    async fn update_api_keys_rejected_for_power_user() {
        let settings = settings_with_user("pk6", true);
        let actor = ProtectedSettingsActor::new(settings).start();

        let result = actor
            .send(UpdateUserApiKeys {
                pubkey: "pk6".to_string(),
                api_keys: ApiKeys::default(),
            })
            .await
            .unwrap();
        assert!(result.is_err());
    }

    #[actix::test]
    async fn get_user_returns_none_for_unknown_pubkey() {
        let settings = ProtectedSettings::default();
        let actor = ProtectedSettingsActor::new(settings).start();

        let user = actor
            .send(GetUser {
                pubkey: "ghost".to_string(),
            })
            .await
            .unwrap();
        assert!(user.is_none());
    }

    #[actix::test]
    async fn save_settings_writes_json_to_tempdir() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("settings.json");
        let path_str = path.to_str().unwrap().to_string();

        let settings = ProtectedSettings::default();
        let actor = ProtectedSettingsActor::new(settings).start();

        let result = actor
            .send(SaveSettings {
                path: path_str.clone(),
            })
            .await
            .unwrap();
        assert!(result.is_ok(), "SaveSettings failed: {:?}", result);
        assert!(path.exists(), "settings.json was not created");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("bindAddress") || content.contains("bind_address"),
            "Expected network field in serialised output"
        );
    }
}
