mod common;
use openwebui_acp::projection::*;
use serde_json::json;
#[test]
fn projection_preserves_access_gates_terminal_and_toggleable_filters() {
    let mut data = common::model_fixture();
    data["meta"]["defaultFeatureIds"] =
        json!(["web_search", "code_interpreter", "image_generation"]);
    data["meta"]["defaultFilterIds"] = json!(["allowed-filter", "not-visible"]);
    data["meta"]["terminalId"] = json!("terminal-fixture");
    data["meta"]["toolIds"] = json!([common::TOOL, common::TOOL]);
    let model: WorkspaceModel = serde_json::from_value(data).unwrap();
    let effective: EffectiveModel =
        serde_json::from_value(json!({"id":common::MODEL,"filters":[{"id":"allowed-filter"}]}))
            .unwrap();
    let tools = vec![Tool {
        id: common::TOOL.into(),
        authenticated: None,
    }];
    let terminals = vec![Terminal {
        id: "terminal-fixture".into(),
        contexts: Default::default(),
    }];
    let server:ServerConfig=serde_json::from_value(json!({"features":{"enable_web_search":true,"enable_code_interpreter":false,"enable_image_generation":true,"enable_memories":true}})).unwrap();
    let user:UserContext=serde_json::from_value(json!({"role":"user","permissions":{"features":{"web_search":false,"image_generation":true,"code_interpreter":true}}})).unwrap();
    let base = "http://127.0.0.1/".parse().unwrap();
    let projected = model
        .project(
            ProjectionContext {
                effective: &effective,
                tools: &tools,
                terminals: &terminals,
                server: &server,
                user: &user,
                settings: &UserSettings::default(),
                memory_default: None,
            },
            &[],
            &base,
        )
        .unwrap();
    assert_eq!(projected.tool_ids, vec![common::TOOL]);
    assert_eq!(projected.terminal_id.as_deref(), Some("terminal-fixture"));
    assert_eq!(projected.filter_ids, vec!["allowed-filter"]);
    assert!(!projected.features.web_search);
    assert!(!projected.features.code_interpreter);
    assert!(projected.features.image_generation);
    assert!(projected.features.memory);
    let settings: UserSettings = serde_json::from_value(json!({"ui":{"memory":false}})).unwrap();
    assert!(
        !model
            .project(
                ProjectionContext {
                    effective: &effective,
                    tools: &tools,
                    terminals: &terminals,
                    server: &server,
                    user: &user,
                    settings: &settings,
                    memory_default: None
                },
                &[],
                &base
            )
            .unwrap()
            .features
            .memory
    );
    let terminals = vec![Terminal {
        id: "terminal-fixture".into(),
        contexts: std::collections::BTreeMap::from([("chat".into(), json!(false))]),
    }];
    assert!(
        model
            .project(
                ProjectionContext {
                    effective: &effective,
                    tools: &tools,
                    terminals: &terminals,
                    server: &server,
                    user: &user,
                    settings: &settings,
                    memory_default: None
                },
                &[],
                &base
            )
            .is_err()
    );
}

#[test]
fn absent_capabilities_do_not_enable_features_and_legacy_is_rejected() {
    let mut data = common::model_fixture();
    data["meta"]["capabilities"] = json!({});
    data["params"]["function_calling"] = json!("legacy");
    let model: WorkspaceModel = serde_json::from_value(data).unwrap();
    assert!(model.validate(common::MODEL).is_err());
    let mut data = common::model_fixture();
    data["meta"]["toolIds"] = json!("invalid");
    assert!(serde_json::from_value::<WorkspaceModel>(data).is_err());
}
