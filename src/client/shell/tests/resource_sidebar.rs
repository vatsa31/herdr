use super::*;

fn resource_state() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut snapshot = snapshot();
    snapshot
        .plugin_resources
        .push(crate::api::schema::PluginResourceCollection {
            plugin_id: "herdr-jira".into(),
            resource_id: "my-issues".into(),
            label: "My Jira issues".into(),
            items: (0..40)
                .map(|i| crate::api::schema::PluginResourceItem {
                    id: format!("MOCK-{i}"),
                    primary: "An issue".into(),
                    secondary: "To Do".into(),
                    payload: None,
                })
                .collect(),
            matched_count: Some(40),
            truncated: false,
            fetched_unix_ms: Some(1),
            loading: false,
            refreshing: false,
            error: None,
            stale: false,
        });
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.sidebar_collapsed = false;
    state.sidebar_collapsed_manual = true;
    state.plugin_resource_collapsed = false;
    state.plugin_resource_collapsed_manual = true;
    state
}

fn click(state: &mut ClientShellState, rect: Rect) {
    assert!(!rect.is_empty(), "control must remain clickable");
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
            kind,
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        })]);
    }
}

#[test]
fn resource_sidebar_collapse_keeps_visible_header_and_reopens() {
    let mut state = resource_state();
    for _ in 0..3 {
        state.compose(106, 30).expect("expanded frame");
        let header = state.hits.resource_header;
        click(&mut state, header);
        assert!(state.plugin_resource_collapsed);
        let frame = state.compose(106, 30).expect("collapsed frame");
        let text: String = frame
            .cells
            .iter()
            .map(|cell| cell.symbol.as_str())
            .collect();
        assert!(text.contains("▸ My Jira issues"));
        assert!(state.hits.resource_items.is_empty());
        let header = state.hits.resource_header;
        click(&mut state, header);
        assert!(!state.plugin_resource_collapsed);
    }
}

#[test]
fn resource_sidebar_leaves_footer_toggle_clickable_in_both_states() {
    for collapsed in [false, true] {
        let mut state = resource_state();
        state.plugin_resource_collapsed = collapsed;
        state.compose(106, 30).expect("frame");
        if !collapsed {
            assert!(!state.hits.resource_scrollbar.is_empty());
        }
        let toggle = state.hits.sidebar_toggle;
        for rect in [
            state.hits.resource_body,
            state.hits.resource_scrollbar,
            state.hits.resource_header,
            state.hits.resource_refresh,
        ] {
            assert!(
                rect.intersection(toggle).is_empty(),
                "resource overlaps footer"
            );
        }
        click(&mut state, toggle);
        assert!(state.sidebar_collapsed);
        assert_eq!(state.plugin_resource_collapsed, collapsed);
        state.compose(106, 30).expect("collapsed sidebar frame");
        let toggle = state.hits.sidebar_toggle;
        click(&mut state, toggle);
        assert!(!state.sidebar_collapsed);
    }
}

#[test]
fn resource_sidebar_short_layout_keeps_header_above_footer() {
    for height in 2..12 {
        for expanded in [false, true] {
            let area = Rect::new(3, 4, 30, height);
            let layout = crate::ui::expanded_sidebar_layout(area, 0.5, true, expanded);
            let resource = layout.resource.expect("resource header space");
            assert!(resource.height >= 1);
            assert!(resource.bottom() < area.bottom());
        }
    }
}
