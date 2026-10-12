use std::sync::Arc;

use reqwest::{Method, Url, header};
use serde::{Serialize, de::DeserializeOwned};
use tokio::time::sleep;

use crate::{
    config::Config,
    error::{Error, Result},
    projection::*,
};

const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct OpenWebUi {
    http: reqwest::Client,
    pub config: Arc<Config>,
}

impl OpenWebUi {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        if config.api_key.expose().is_empty() || config.api_key.expose().starts_with("file:") {
            return Err(Error::Config(
                "Load the credential file before creating an API client".into(),
            ));
        }
        let mut auth =
            header::HeaderValue::from_str(&format!("Bearer {}", config.api_key.expose()))
                .map_err(|_| Error::Config("Invalid API key header value".into()))?;
        auth.set_sensitive(true);
        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, auth);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(
                config
                    .request_timeout
                    .min(std::time::Duration::from_secs(10)),
            )
            .timeout(config.request_timeout)
            .user_agent(concat!("openwebui-acp/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| Error::Config("Could not initialize HTTP client".into()))?;
        Ok(Self { http, config })
    }

    pub fn endpoint(&self, segments: &[&str]) -> Url {
        let mut url = self.config.url.clone();
        url.path_segments_mut()
            .expect("validated HTTP base URL")
            .pop_if_empty()
            .extend(segments);
        url
    }

    pub async fn get<T: DeserializeOwned>(&self, url: Url, operation: &'static str) -> Result<T> {
        self.request::<(), T>(Method::GET, url, None, operation)
            .await
    }

    pub async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        url: Url,
        body: &B,
        operation: &'static str,
    ) -> Result<T> {
        self.request(Method::POST, url, Some(body), operation).await
    }

    async fn request<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<&B>,
        operation: &'static str,
    ) -> Result<T> {
        if url.origin() != self.config.url.origin()
            || !url.path().starts_with(self.config.url.path())
        {
            return Err(Error::Config(
                "Refusing to send Open WebUI credentials outside the configured origin/base path"
                    .into(),
            ));
        }
        let encoded = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| Error::InvalidResponse("request encoding"))?;
        if encoded
            .as_ref()
            .is_some_and(|b| b.len() > MAX_RESPONSE_BYTES)
        {
            return Err(Error::Capacity);
        }
        let attempts = if method == Method::GET { 3 } else { 1 };
        for attempt in 0..attempts {
            let mut request = self.http.request(method.clone(), url.clone());
            if let Some(body) = &encoded {
                request = request
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(body.clone());
            }
            let response = request.send().await;
            let transient = match &response {
                Err(e) => e.is_connect() || e.is_timeout(),
                Ok(r) => matches!(r.status().as_u16(), 429 | 502 | 503 | 504),
            };
            if transient && attempt + 1 < attempts {
                sleep(std::time::Duration::from_millis(200 * (1 << attempt))).await;
                continue;
            }
            let mut response = response.map_err(|_| Error::Network(operation))?;
            if !response.status().is_success() {
                return Err(Error::Http {
                    operation,
                    status: response.status().as_u16(),
                });
            }
            if response
                .content_length()
                .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
            {
                return Err(Error::InvalidResponse(operation));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| Error::Network(operation))?
            {
                if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                    return Err(Error::InvalidResponse(operation));
                }
                bytes.extend_from_slice(&chunk);
            }
            return serde_json::from_slice(&bytes).map_err(|_| Error::InvalidResponse(operation));
        }
        unreachable!("at least one request attempt")
    }

    pub async fn defaults(&self) -> Result<(WorkspaceModel, Defaults)> {
        let mut model_url = self.endpoint(&["api", "v1", "models", "model"]);
        model_url
            .query_pairs_mut()
            .append_pair("id", &self.config.model);
        let model: WorkspaceModel = self.get(model_url, "Workspace model metadata").await?;
        model.validate(&self.config.model)?;
        let (models, tools, server, mut user, permissions, settings) = tokio::try_join!(
            self.get::<EffectiveModels>(self.endpoint(&["api", "models"]), "effective models"),
            self.get::<Vec<Tool>>(
                self.endpoint(&["api", "v1", "tools", ""]),
                "tool availability"
            ),
            self.get::<ServerConfig>(
                self.endpoint(&["api", "config"]),
                "server feature configuration"
            ),
            self.get::<UserContext>(
                self.endpoint(&["api", "v1", "users", "user", "status"]),
                "account profile (Open WebUI user status must be enabled)"
            ),
            self.get::<Permissions>(
                self.endpoint(&["api", "v1", "users", "permissions"]),
                "effective account permissions"
            ),
            self.get::<Option<UserSettings>>(
                self.endpoint(&["api", "v1", "users", "user", "settings"]),
                "account UI defaults"
            ),
        )?;
        user.permissions = permissions;
        let effective = models
            .data
            .iter()
            .find(|m| m.id == model.id)
            .ok_or_else(|| {
                Error::Model(
                    "Workspace model is missing from the account's effective model list".into(),
                )
            })?;
        let terminals = if model
            .meta
            .terminal_id
            .as_ref()
            .is_some_and(|id| !id.is_empty())
        {
            self.get::<Vec<Terminal>>(
                self.endpoint(&["api", "v1", "terminals", ""]),
                "terminal availability",
            )
            .await?
        } else {
            Vec::new()
        };
        let defaults = model.project(
            ProjectionContext {
                effective,
                tools: &tools,
                terminals: &terminals,
                server: &server,
                user: &user,
                settings: &settings.unwrap_or_default(),
                memory_default: self.config.memory_default,
            },
            &self.config.required_tools,
            &self.config.url,
        )?;
        Ok((model, defaults))
    }

    pub async fn account_id(&self) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Identity {
            id: String,
        }
        let identity: Identity = self
            .get(
                self.endpoint(&["api", "v1", "users", "user", "status"]),
                "account identity (Open WebUI user status must be enabled)",
            )
            .await?;
        if identity.id.is_empty() || identity.id.len() > 512 {
            return Err(Error::InvalidResponse("account identity"));
        }
        Ok(identity.id)
    }
}
