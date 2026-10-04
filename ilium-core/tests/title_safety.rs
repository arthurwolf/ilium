use ilium_core::{
    NodeKind, PaneContentKind, PaneTitleSource, RestructureNode, RestructurePlan, Tree, ROOT_ID,
};

#[test]
fn ai_restructure_does_not_claim_user_title_ownership() {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "Initial").unwrap();
    let pane = tree
        .add_pane(group, "Shell", PaneContentKind::Terminal)
        .unwrap();
    tree.apply_restructure(RestructurePlan {
        children: vec![RestructureNode::Group {
            title: "Tasks".into(),
            short_title: None,
            icon: None,
            children: vec![RestructureNode::Pane {
                id: pane,
                title: "Actual task".into(),
                short_title: Some("Task".into()),
                icon: Some("🧪".into()),
            }],
        }],
    })
    .unwrap();
    let NodeKind::Pane { title_source, .. } = &tree.get(pane).unwrap().kind else {
        panic!("pane");
    };
    assert_eq!(*title_source, PaneTitleSource::Automatic);
    assert!(!tree.get(pane).unwrap().is_name_fixed);
}

#[test]
fn regrouping_keeps_genuine_manual_title_ownership() {
    let mut tree = Tree::new();
    let group = tree.add_group(ROOT_ID, "Initial").unwrap();
    let pane = tree
        .add_pane(group, "Shell", PaneContentKind::Terminal)
        .unwrap();
    tree.rename_node(pane, "My research", Some("Mine".into()), Some("📌".into()))
        .unwrap();
    tree.apply_restructure(RestructurePlan {
        children: vec![RestructureNode::Group {
            title: "Tasks".into(),
            short_title: None,
            icon: None,
            children: vec![RestructureNode::Pane {
                id: pane,
                title: "Unrelated task".into(),
                short_title: None,
                icon: None,
            }],
        }],
    })
    .unwrap();
    let node = tree.get(pane).unwrap();
    assert_eq!(node.name, "My research");
    assert_eq!(node.short_name.as_deref(), Some("Mine"));
    assert_eq!(node.inferred_icon.as_deref(), Some("📌"));
    assert!(node.is_name_fixed);
    let NodeKind::Pane { title_source, .. } = &node.kind else {
        panic!("pane");
    };
    assert_eq!(*title_source, PaneTitleSource::UserSpecified);
}
