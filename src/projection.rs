use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

#[derive(Clone, Deserialize)]
pub struct WorkspaceModel {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub params: BTreeMap<String, Value>,
    pub meta: ModelMeta,
}

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMeta {
    pub tool_ids: Option<Vec<String>>,
    pub terminal_id: Option<String>,
    pub default_feature_ids: Option<Vec<String>>,
    pub default_filter_ids: Option<Vec<String>>,
    pub capabilities: Option<BTreeMap<String, bool>>,
}

#[derive(Deserialize)]
pub struct Tool {
    pub id: String,
    pub authenticated: Option<bool>,
}

#[derive(Deserialize)]
pub struct EffectiveModels {
    pub data: Vec<EffectiveModel>,
}

#[derive(Deserialize)]
pub struct EffectiveModel {
    pub id: String,
    #[serde(default)]
    pub filters: Vec<Filter>,
}

#[derive(Deserialize)]
pub struct Filter {
    pub id: String,
}

#[derive(Deserialize)]
pub struct Terminal {
    pub id: String,
    #[serde(default)]
    pub contexts: BTreeMap<String, Value>,
}

#[derive(Default, Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub features: GlobalFeatures,
}

// /api/config also contains counts and strings; ignore those unrelated fields.
#[derive(Default, Deserialize)]
pub struct GlobalFeatures {
    pub enable_web_search: Option<bool>,
    pub enable_code_interpreter: Option<bool>,
    pub enable_image_generation: Option<bool>,
    pub enable_memories: Option<bool>,
}

impl GlobalFeatures {
    fn get(&self, name: &str) -> Option<bool> {
        match name {
            "web_search" => self.enable_web_search,
            "code_interpreter" => self.enable_code_interpreter,
            "image_generation" => self.enable_image_generation,
            _ => None,
        }
    }
}

pub struct ProjectionContext<'a> {
    pub effective: &'a EffectiveModel,
    pub tools: &'a [Tool],
    pub terminals: &'a [Terminal],
    pub server: &'a ServerConfig,
    pub user: &'a UserContext,
    pub settings: &'a UserSettings,
    pub memory_default: Option<bool>,
}

#[derive(Deserialize)]
pub struct UserContext {
    pub role: String,
    #[serde(default)]
    pub permissions: Permissions,
}

#[derive(Default, Deserialize)]
pub struct Permissions {
    #[serde(default)]
    pub features: BTreeMap<String, bool>,
}

#[derive(Default, Deserialize)]
pub struct UserSettings {
    #[serde(default, deserialize_with = "null_default")]
    pub ui: UiSettings,
}

fn null_default<'de, D: serde::Deserializer<'de>, T: Deserialize<'de> + Default>(
    deserializer: D,
) -> std::result::Result<T, D::Error> {
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Default, Deserialize)]
pub struct UiSettings {
    pub memory: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Features {
    pub web_search: bool,
    pub code_interpreter: bool,
    pub image_generation: bool,
    pub memory: bool,
}

#[derive(Clone, Debug)]
pub struct Defaults {
    pub tool_ids: Vec<String>,
    pub terminal_id: Option<String>,
    pub filter_ids: Vec<String>,
    pub features: Features,
}

impl WorkspaceModel {
    pub fn validate(&self, expected: &str) -> Result<()> {
        if self.id != expected || self.is_active == Some(false) {
            return Err(Error::Model(
                "Selected Workspace model is inactive or does not match the requested ID".into(),
            ));
        }
        if self
            .params
            .get("function_calling")
            .and_then(Value::as_str)
            .is_some_and(|v| v != "native")
        {
            return Err(Error::Model("Native function calling is required; change the model in Open WebUI after approval".into()));
        }
        Ok(())
    }

    pub fn project(
        &self,
        context: ProjectionContext<'_>,
        required: &[String],
        base_url: &reqwest::Url,
    ) -> Result<Defaults> {
        let ProjectionContext {
            effective,
            tools,
            terminals,
            server,
            user,
            settings,
            memory_default,
        } = context;
        let tool_ids = unique(self.meta.tool_ids.as_deref().unwrap_or_default());
        for id in required {
            if !tool_ids.contains(id) {
                return Err(Error::Model("A required tool is absent from the Workspace model's defaults; update the model in Open WebUI after approval".into()));
            }
        }
        // Never silently discard a configured default: doing so would change the model.
        for id in &tool_ids {
            let tool = tools.iter().find(|t| t.id == *id).ok_or_else(|| {
                Error::Model("A model-default tool is inaccessible to this account".into())
            })?;
            if tool.authenticated == Some(false) {
                let mut url = base_url.clone();
                if let Some(server_id) = id.strip_prefix("server:mcp:") {
                    url.path_segments_mut()
                        .map_err(|_| Error::InvalidResponse("OAuth URL"))?
                        .pop_if_empty()
                        .extend(["oauth", "clients", &format!("mcp:{server_id}"), "authorize"]);
                }
                return Err(Error::Authentication { url: url.into() });
            }
        }
        let caps = self.meta.capabilities.as_ref();
        let enabled = |name: &str| -> bool {
            self.meta
                .default_feature_ids
                .as_ref()
                .is_some_and(|v| v.iter().any(|f| f == name))
                && caps.and_then(|c| c.get(name)).copied().unwrap_or(false)
                // API keys may see only public /api/config fields. These three
                // switches also have verified backend global gates, so leave
                // unknown global availability for Open WebUI to enforce.
                && server.features.get(name).unwrap_or(true)
                && (user.role == "admin"
                    || user
                        .permissions
                        .features
                        .get(name)
                        .copied()
                        .unwrap_or(false))
        };
        let terminal_id = self
            .meta
            .terminal_id
            .clone()
            .filter(|id| !id.is_empty())
            .filter(|_| {
                caps.and_then(|c| c.get("terminal"))
                    .copied()
                    .unwrap_or(true)
            });
        if let Some(id) = &terminal_id {
            let terminal = terminals.iter().find(|t| t.id == *id).ok_or_else(|| {
                Error::Model("The model's default terminal is inaccessible".into())
            })?;
            if terminal.contexts.get("chat") == Some(&Value::Bool(false)) {
                return Err(Error::Model(
                    "The default terminal is not available in chat context".into(),
                ));
            }
        }
        let filter_ids = unique(self.meta.default_filter_ids.as_deref().unwrap_or_default())
            .into_iter()
            .filter(|id| effective.filters.iter().any(|f| f.id == *id))
            .collect();
        Ok(Defaults {
            tool_ids,
            terminal_id,
            filter_ids,
            features: Features {
                web_search: enabled("web_search"),
                code_interpreter: enabled("code_interpreter"),
                image_generation: enabled("image_generation"),
                // Memory's global setting is a UI default, not the same backend
                // gate as web search. Do not guess when the API key cannot see it.
                memory: settings.ui.memory.or(server.features.enable_memories).or(memory_default)
                    .ok_or_else(|| Error::Model("The API does not expose the memory default. Check Memory in the Open WebUI browser, then set OPENWEBUI_ACP_MEMORY_DEFAULT=true or false to match it".into()))?,
            },
        })
    }
}

fn unique(values: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for v in values {
        if !result.contains(v) {
            result.push(v.clone());
        }
    }
    result
}
