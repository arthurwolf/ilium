//! Regressions for the always-on recommendation contract, independent of
//! whether the user has selected Semantic for actual rendering.

use super::*;

fn context() -> LeafContext {
    LeafContext {
        id: NodeId(1),
        kind_label: "Plain shell".into(),
        current_title: "Paris transport research".into(),
        current_icon: None,
        is_name_fixed: false,
        filename: None,
        content_extract: "Working on Paris public transport maps".into(),
        automatic_content_fingerprint: 0,
        agent_lookup: None,
    }
}

#[test]
fn semantic_contract_prompt_includes_every_concrete_animation() {
    let prompt = render_restructure_prompt(TitleStyle::Summarization, &[context()], "", &[], None)
        .expect("the small real project context renders");
    let catalog = prompt
        .split_once("<animation-catalog>")
        .and_then(|(_, tail)| tail.split_once("</animation-catalog>"))
        .map(|(catalog, _)| catalog)
        .expect("every reorganization call includes the animation catalog");
    for id in [
        "shoreline",
        "moonlit_water",
        "sleeping_ridge",
        "windy_hillside",
        "tea_steam",
        "kelp",
        "stone_caustics",
        "cloudlets",
        "two_ripples",
        "quiet_pond",
        "pipes",
        "stars",
        "night_lights",
        "clouds",
        "video",
        "spectrum",
        "images",
        "dither_water",
        "atlantic_dusk",
        "cube_clock",
        "box_machine",
        "machine_screen",
        "fbm_clouds",
        "dithered_waves",
        "dithr_patterns",
        "hex_expedition",
        "vector_td",
        "wikipedia",
        "galactic_empires",
        "voxel_landscape",
        "solar_system",
        "topographic_maps",
        "graph",
        "pi",
        "earthquakes",
        "aircraft",
        "boats",
        "chess",
        "open_street_map",
        "carpet",
    ] {
        assert!(
            catalog
                .lines()
                .any(|line| line.starts_with(&format!("{id:?} | "))),
            "missing animation {id}"
        );
    }
    assert!(
        catalog.contains("Paris"),
        "the model can choose the existing offline Paris map"
    );
    assert!(
        catalog.contains("Snake"),
        "the model can choose Carpet's Snake subanimation"
    );
}

#[test]
fn semantic_contract_rejects_title_only_restructure_even_when_feature_is_disabled() {
    let result = parse_restructure_response(
        r#"{"children":[{"kind":"pane","id":1,"title":"Paris Transport","icon":"P"}]}"#,
        &[context()],
        &[],
        TitleStyle::Summarization,
        &RecommendationContext::default(),
    );
    assert!(
        result.is_err(),
        "animation recommendations are required on every reorganization reply"
    );
}

#[test]
fn semantic_contract_rejects_invented_animation_instead_of_ignoring_it() {
    let result = parse_restructure_response(
        r#"{"animation":{"animation_id":"invented_animation","parameters":[]},"children":[{"kind":"pane","id":1,"title":"Paris Transport","icon":"P","animation":{"animation_id":"invented_animation","parameters":[]}}]}"#,
        &[context()],
        &[],
        TitleStyle::Summarization,
        &RecommendationContext::default(),
    );
    assert!(
        result.is_err(),
        "unknown animation pointers must receive corrective feedback"
    );
}

fn valid_response() -> serde_json::Value {
    serde_json::json!({"animations":{"definitions":[{"key":"map","recommendation":{"kind":"open_street_map","resources":"catalog","parameters":[{"id":"source","value":{"Index":0}},{"id":"place","value":{"Index":0}},{"id":"tour","value":{"Index":0}}]}},{"key":"snake","recommendation":{"kind":"carpet","resources":"catalog","parameters":[{"id":"carpet_mode","value":{"Index":1}}]}}],"project":"map"},"children":[{"kind":"group","title":"Paris work","animation":"map","children":[{"kind":"pane","id":1,"title":"Paris transport","animation":"snake"}]}]})
}
fn parse_value(value: &serde_json::Value) -> anyhow::Result<RecommendedRestructurePlan> {
    parse_restructure_response(
        &value.to_string(),
        &[context()],
        &[],
        TitleStyle::Summarization,
        &RecommendationContext::default(),
    )
}
#[test]
fn semantic_contract_resolves_project_group_and_leaf() {
    let plan = parse_value(&valid_response()).unwrap();
    assert_eq!(plan.project.kind, "open_street_map");
    assert_eq!(
        plan.entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>(),
        vec![vec![0], vec![0, 0]]
    );
    assert_eq!(plan.entries[1].recommendation.kind, "carpet");
    assert_eq!(plan.expected_animation_generation, 0);
}
#[test]
fn semantic_contract_rejects_missing_invalid_duplicate_and_unused_definitions() {
    for case in 0..9 {
        let mut value = valid_response();
        match case {
            0 => {
                value["animations"]
                    .as_object_mut()
                    .unwrap()
                    .remove("project");
            }
            1 => {
                value["children"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("animation");
            }
            2 => {
                value["children"][0]["children"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("animation");
            }
            3 => value["children"][0]["animation"] = serde_json::json!("missing"),
            4 => {
                value["animations"]["definitions"][0]["recommendation"]["kind"] =
                    serde_json::json!("invented_animation")
            }
            5 => {
                value["animations"]["definitions"][1]["recommendation"]["parameters"][0]["value"] =
                    serde_json::json!({"Index":9})
            }
            6 => {
                let duplicate = value["animations"]["definitions"][0].clone();
                value["animations"]["definitions"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            7 => {
                let mut unused = value["animations"]["definitions"][0].clone();
                unused["key"] = serde_json::json!("unused");
                value["animations"]["definitions"]
                    .as_array_mut()
                    .unwrap()
                    .push(unused);
            }
            _ => {
                value["animations"]["definitions"][0]["recommendation"]
                    .as_object_mut()
                    .unwrap()
                    .remove("resources");
            }
        }
        assert!(
            parse_value(&value).is_err(),
            "accepted malformed case {case}"
        );
    }
}
#[test]
fn semantic_contract_preserves_fixed_groups_and_split_paths() {
    let mut value = valid_response();
    let pane = value["children"][0]["children"][0].clone();
    value["children"] = serde_json::json!([{"kind":"existing_group","id":9,"animation":"map","children":[{"kind":"split_view","id":10,"animation":"map","children":[pane]}]}]);
    let context = RecommendationContext {
        snapshot: RecommendationSnapshot {
            expected_animation_generation: 7,
            fixed_groups: vec![(NodeId(9), "User group".into())],
        },
        authored: Default::default(),
    };
    let splits = [ProtectedSplitViewContext {
        id: NodeId(10),
        orientation: SplitOrientation::Horizontal,
        current_title: "User split".into(),
        ordered_pane_ids: vec![NodeId(1)],
    }];
    let plan = parse_restructure_response(
        &value.to_string(),
        &[self::context()],
        &splits,
        TitleStyle::Summarization,
        &context,
    )
    .unwrap();
    assert_eq!(plan.expected_animation_generation, 7);
    assert_eq!(
        plan.entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>(),
        vec![vec![0], vec![0, 0], vec![0, 0, 0]]
    );
    value["children"][0]["id"] = serde_json::json!(8);
    assert!(parse_restructure_response(
        &value.to_string(),
        &[self::context()],
        &splits,
        TitleStyle::Summarization,
        &context
    )
    .is_err());
}
struct RawResponses {
    responses: std::cell::RefCell<std::collections::VecDeque<String>>,
    prompts: std::cell::RefCell<Vec<String>>,
}
impl RestructureCompletionClient for RawResponses {
    fn complete_restructure_prompt(&self, prompt: &str) -> anyhow::Result<String> {
        self.prompts.borrow_mut().push(prompt.to_owned());
        Ok(self
            .responses
            .borrow_mut()
            .pop_front()
            .expect("unexpected extra inference call"))
    }
}
#[test]
fn semantic_contract_retries_with_catalog_and_original_prompt_limit() {
    let invalid = r#"{"children":[{"kind":"pane","id":1,"title":"Paris"}]}"#.to_owned();
    let generator = RawResponses {
        responses: std::cell::RefCell::new(
            [invalid, valid_response().to_string()]
                .into_iter()
                .collect(),
        ),
        prompts: Default::default(),
    };
    let plan = infer_restructure_plan(&generator, &[context()], &RecommendationContext::default())
        .unwrap();
    assert_eq!(plan.entries.len(), 2);
    let prompts = generator.prompts.borrow();
    assert_eq!(prompts.len(), 2);
    let catalog = crate::semantic_animation::catalog().unwrap();
    for prompt in prompts.iter() {
        assert!(prompt.contains(catalog));
        assert!(prompt.chars().count() <= 32_000);
    }
    assert!(prompts[1].contains("<retry-feedback>"));
}
#[test]
fn semantic_contract_budget_retains_fixed_group_and_all_leaf_ids() {
    let mut contexts = (1..=10)
        .map(|id| {
            let mut item = context();
            item.id = NodeId(id);
            item.content_extract = format!("head {} tail", "x".repeat(30_000));
            item
        })
        .collect::<Vec<_>>();
    contexts[0].current_title = "Paris".into();
    let recommendation_context = RecommendationContext {
        snapshot: RecommendationSnapshot {
            expected_animation_generation: 3,
            fixed_groups: vec![(NodeId(900), "Mandatory fixed group".into())],
        },
        authored: Default::default(),
    };
    let prompt = render_restructure_prompt_with_instructions(
        TitleStyle::Summarization,
        &ilium_inference::PromptInstructions::default(),
        &contexts,
        &"y".repeat(30_000),
        &[],
        None,
        &recommendation_context,
    )
    .unwrap();
    assert!(prompt.chars().count() <= 32_000);
    assert!(prompt.contains("<fixed-group id=\"900\""));
    for item in contexts {
        assert!(prompt.contains(&format!("<item id=\"{}\"", item.id.0)));
    }
    assert!(prompt.contains(crate::semantic_animation::catalog().unwrap()));
}
