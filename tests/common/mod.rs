#![allow(dead_code)]

use clap::Parser;
use openwebui_acp::{
    config::{Args, Config},
    openwebui::OpenWebUi,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path, path_regex},
};

pub const MODEL: &str = "workspace-test";
pub const TOOL: &str = "server:mcp:inventory";
pub const KEY: &str = "test-key-never-log";

pub fn config(url: &str) -> Config {
    let args = Args::try_parse_from([
        "harness",
        "--url",
        url,
        "--allow-insecure-http",
        "true",
        "--model",
        MODEL,
        "--require-tool",
        TOOL,
        "--buzz-auto-publish",
        "false",
    ])
    .unwrap();
    let env = HashMap::from([("OPENWEBUI_API_KEY".into(), KEY.into())]);
    let mut config = Config::resolve(args, &env, &HashMap::new()).unwrap();
    config.poll_interval = Duration::from_millis(10);
    config.request_timeout = Duration::from_secs(2);
    config.stop_timeout = Duration::from_secs(1);
    config.turn_timeout = Duration::from_secs(5);
    config
}

pub fn model_fixture() -> Value {
    json!({"id": MODEL,"name":"Fixture model","params":{},"is_active":true,"meta":{
        "toolIds":[TOOL,"server:mcp:calendar","server:mcp:notes"],
        "defaultFeatureIds":["web_search"],"terminalId":null,
        "skillIds":["skill-fixture"],"knowledge":[{"id":"knowledge-fixture"}],
        "capabilities":{"web_search":true,"code_interpreter":true,"image_generation":true,"terminal":true,"builtin_tools":true}
    }})
}

#[derive(Default)]
pub struct State {
    pub chats: HashMap<String, Value>,
    pub completions: usize,
    pub hang: bool,
    pub failed: bool,
    pub unfinished: bool,
    pub stopped: bool,
    pub missing_assistant: bool,
    pub orphan_assistant: bool,
}

pub struct Fixture {
    pub server: MockServer,
    pub state: Arc<Mutex<State>>,
}

impl Fixture {
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        for (url, body) in [
            ("/api/v1/models/model", model_fixture()),
            ("/api/models", json!({"data":[{"id":MODEL,"filters":[]}]})),
            (
                "/api/v1/tools/",
                json!([{"id":TOOL,"authenticated":true},{"id":"server:mcp:calendar"},{"id":"server:mcp:notes","authenticated":true}]),
            ),
            (
                "/api/config",
                json!({"features":{"enable_web_search":true,"enable_code_interpreter":true,"enable_image_generation":true,"enable_memories":false}}),
            ),
            (
                "/api/v1/users/user/status",
                json!({"id":"fixture-account","role":"user"}),
            ),
            (
                "/api/v1/users/permissions",
                json!({"features":{"web_search":true,"code_interpreter":true,"image_generation":false}}),
            ),
            (
                "/api/v1/users/user/settings",
                json!({"ui":{"memory":false}}),
            ),
        ] {
            Mock::given(method("GET"))
                .and(path(url))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .with_priority(100)
                .mount(&server)
                .await;
        }
        let state = Arc::new(Mutex::new(State::default()));
        let s = state.clone();
        Mock::given(method("POST"))
            .and(path("/api/v1/chats/new"))
            .respond_with(move |r: &Request| {
                let data: Value = serde_json::from_slice(&r.body).unwrap();
                let mut s = s.lock().unwrap();
                let id = format!("chat-{}", s.chats.len() + 1);
                let chat = json!({"id":id,"chat":data["chat"]});
                s.chats.insert(id, chat.clone());
                ResponseTemplate::new(200).set_body_json(chat)
            })
            .with_priority(100)
            .mount(&server)
            .await;
        let s = state.clone();
        Mock::given(method("POST"))
            .and(path_regex(r"^/api/v1/chats/chat-\d+$"))
            .respond_with(move |r: &Request| {
                let data: Value = serde_json::from_slice(&r.body).unwrap();
                let id = r.url.path().rsplit('/').next().unwrap();
                let chat = json!({"id":id,"chat":data["chat"]});
                s.lock().unwrap().chats.insert(id.into(), chat.clone());
                ResponseTemplate::new(200).set_body_json(chat)
            })
            .with_priority(100)
            .mount(&server)
            .await;
        let s = state.clone();
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/v1/chats/chat-\d+$"))
            .respond_with(move |r: &Request| {
                let id = r.url.path().rsplit('/').next().unwrap();
                ResponseTemplate::new(200)
                    .set_body_json(s.lock().unwrap().chats.get(id).cloned().unwrap())
            })
            .with_priority(100)
            .mount(&server)
            .await;
        let s = state.clone();
        Mock::given(method("POST")).and(path("/api/chat/completions")).respond_with(move |r: &Request| {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            let mut s = s.lock().unwrap();
            s.completions += 1;
            s.stopped = false;
            let failed = s.failed;
            let done = !s.hang && !s.unfinished;
            let missing = s.missing_assistant;
            let orphan = s.orphan_assistant;
            let id = body["chat_id"].as_str().unwrap();
            let message_id = body["id"].as_str().unwrap();
            let chat = s.chats.get_mut(id).unwrap();
            let m = &mut chat["chat"]["history"]["messages"][message_id];
            // Open WebUI rebuilds the assistant placeholder from user_message.id,
            // even when the caller already saved a correctly linked placeholder.
            m["parentId"] = if orphan { Value::Null } else { body["user_message"]["id"].clone() };
            m["content"] = json!("ORCHARD");
            m["done"] = json!(done);
            m["output"] = json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ORCHARD"}]}]);
            m["sources"] = json!([{"source":{"id":"server-owned-source"}}]);
            if failed { m["error"] = json!({"detail":"SERVER_SECRET_NOT_FOR_LOGS"}); }
            if missing { chat["chat"]["history"]["messages"].as_object_mut().unwrap().remove(message_id); }
            ResponseTemplate::new(200).set_body_json(json!({"status":true,"task_ids":["task"],"chat_id":id}))
        }).with_priority(100).mount(&server).await;
        let s = state.clone();
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/tasks/chat/chat-\d+$"))
            .respond_with(move |_: &Request| {
                let s = s.lock().unwrap();
                ResponseTemplate::new(200).set_body_json(
                    json!({"task_ids":if s.hang && !s.stopped {vec!["task"]} else {vec![]}}),
                )
            })
            .with_priority(100)
            .mount(&server)
            .await;
        let s = state.clone();
        Mock::given(method("POST"))
            .and(path_regex(r"^/api/tasks/chat/chat-\d+/stop$"))
            .respond_with(move |_: &Request| {
                s.lock().unwrap().stopped = true;
                ResponseTemplate::new(200).set_body_json(json!({"status":true}))
            })
            .with_priority(100)
            .mount(&server)
            .await;
        Self { server, state }
    }

    pub fn api(&self) -> OpenWebUi {
        OpenWebUi::new(Arc::new(config(&self.server.uri()))).unwrap()
    }

    pub async fn completions(&self) -> Vec<Value> {
        self.server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.url.path() == "/api/chat/completions")
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }
}
