use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::render::{display_width, put_text};
use super::*;

pub(super) fn render_plugin_resource_panel(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    collapsed: bool,
    selected: &mut Option<String>,
    scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let Some(resource) = snapshot.plugin_resources.first() else {
        return;
    };
    // v1 shows the first enabled resource only; the server may poll more.
    // Prune a selection that no longer exists so a removed issue is not
    // highlighted as a ghost (activation already errors server-side).
    if let Some(id) = selected.as_deref() {
        if !resource.items.iter().any(|item| item.id == id) {
            *selected = None;
        }
    }
    let palette = &config.palette;
    let divider_height = u16::from(area.height > 1);
    if divider_height > 0 {
        put_text(
            buffer,
            area.x,
            area.y,
            area.width,
            &"─".repeat(area.width as usize),
            Style::default().fg(palette.surface_dim),
        );
    }
    hits.resource_section_divider = Rect::new(area.x, area.y, area.width, divider_height);
    let marker = if collapsed { "▸" } else { "▾" };
    let count = resource_count_label(resource);
    let title = format!(" {marker} {} · {count}", resource.label);
    // A one-row collapsed (or height-constrained) panel still needs its header.
    let header = Rect::new(area.x, area.y + divider_height, area.width, 1);
    hits.resource_header = header;
    put_text(
        buffer,
        header.x,
        header.y,
        header.width.saturating_sub(8),
        &title,
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    let refresh_label = if resource.refreshing { "…" } else { "↻" };
    let refresh_width = (display_width(refresh_label) as u16)
        .saturating_add(1)
        .min(header.width);
    let refresh = Rect::new(
        header.right().saturating_sub(refresh_width),
        header.y,
        refresh_width,
        1,
    );
    hits.resource_refresh = refresh;
    put_text(
        buffer,
        refresh.x,
        refresh.y,
        refresh.width,
        refresh_label,
        Style::default().fg(palette.accent),
    );
    if collapsed {
        return;
    }

    let body = Rect::new(
        area.x,
        header.bottom(),
        area.width,
        area.bottom().saturating_sub(header.bottom()),
    );
    hits.resource_body = body;
    if body.is_empty() {
        return;
    }

    let status = resource_status_line(resource);
    if let Some(status) =
        status.filter(|_| resource.items.is_empty() || resource.stale || resource.loading)
    {
        put_text(
            buffer,
            body.x,
            body.y,
            body.width,
            &status,
            Style::default()
                .fg(if resource.error.is_some() {
                    palette.red
                } else {
                    palette.overlay0
                })
                .add_modifier(Modifier::DIM),
        );
        if resource.items.is_empty() {
            *scroll = 0;
            return;
        }
    }

    let list_y = if resource.stale || resource.loading && !resource.items.is_empty() {
        body.y.saturating_add(1)
    } else {
        body.y
    };
    let list = Rect::new(
        body.x,
        list_y,
        body.width,
        body.bottom().saturating_sub(list_y),
    );
    if list.is_empty() {
        return;
    }

    let row_heights = std::iter::repeat_n(2u16, resource.items.len()).collect::<Vec<_>>();
    let gaps = vec![0u16; resource.items.len()];
    let metrics = super::scroll::list_scroll_metrics(&row_heights, &gaps, list.height, *scroll);
    hits.resource_max_scroll = metrics.max_offset_from_bottom;
    hits.resource_scroll_metrics = Some(metrics);
    *scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    // Keep the selected item visible across reorder/refresh where possible.
    if let Some(id) = selected.as_deref() {
        if let Some(index) = resource.items.iter().position(|item| item.id == id) {
            let visible_rows = usize::from(list.height).saturating_add(1) / 2;
            let visible_rows = visible_rows.max(1);
            if index < *scroll {
                *scroll = index;
            } else if index >= scroll.saturating_add(visible_rows) {
                *scroll = index.saturating_add(1).saturating_sub(visible_rows);
            }
        }
    }
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && list.width > 1;
    let content_width = list.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = list.y;
    for (index, item) in resource.items.iter().enumerate().skip(*scroll) {
        if y.saturating_add(2) > list.bottom() {
            break;
        }
        let rect = Rect::new(
            list.x,
            y,
            content_width,
            2.min(list.bottom().saturating_sub(y)),
        );
        let selected_row = selected.as_deref() == Some(item.id.as_str());
        if selected_row {
            buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
        }
        let primary = format!("{}  {}", item.id, item.primary);
        put_text(
            buffer,
            rect.x.saturating_add(1),
            rect.y,
            rect.width.saturating_sub(1),
            &crate::ui::truncate_end(&primary, rect.width.saturating_sub(1) as usize),
            Style::default().fg(palette.text),
        );
        if rect.height > 1 {
            put_text(
                buffer,
                rect.x.saturating_add(1),
                rect.y + 1,
                rect.width.saturating_sub(1),
                &crate::ui::truncate_end(&item.secondary, rect.width.saturating_sub(1) as usize),
                Style::default().fg(palette.overlay0),
            );
        }
        hits.resource_items.push((
            rect,
            resource.plugin_id.clone(),
            resource.resource_id.clone(),
            item.id.clone(),
        ));
        y = y.saturating_add(row_heights[index]);
    }
    if show_scrollbar {
        let track = Rect::new(list.right().saturating_sub(1), list.y, 1, list.height);
        hits.resource_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }
}

fn resource_count_label(resource: &crate::api::schema::PluginResourceCollection) -> String {
    if resource.loading && resource.items.is_empty() {
        return "…".into();
    }
    if let Some(error) = resource
        .error
        .as_ref()
        .filter(|_| resource.items.is_empty())
    {
        let _ = error;
        return "!".into();
    }
    if resource.truncated {
        if let Some(matched) = resource.matched_count {
            return matched.to_string();
        }
        return format!("{}+", resource.items.len());
    }
    resource
        .matched_count
        .map(|count| count.to_string())
        .unwrap_or_else(|| resource.items.len().to_string())
}

fn resource_status_line(resource: &crate::api::schema::PluginResourceCollection) -> Option<String> {
    if resource.loading && resource.items.is_empty() {
        return Some(" loading…".into());
    }
    if let Some(error) = &resource.error {
        if resource.stale {
            return Some(format!(" stale: {error}"));
        }
        return Some(format!(" {error}"));
    }
    if resource.items.is_empty() {
        return Some(" no matching items".into());
    }
    None
}
