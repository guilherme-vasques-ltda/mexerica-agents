// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The usage analytics sheet.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::super::manage::AnalyticsSheet;
use super::super::md;
use super::buffer::{centered, frame_box, hline, put, put_line, u};
use super::style::theme;

/// A coleta de analytics permanece desativada no fork.
const DETAILS: [(&str, &str); 1] = [("Dados de uso", "A coleta de analytics está desativada.")];

pub(super) fn analytics(buf: &mut Buffer, area: Rect, s: &AnalyticsSheet) {
    let t = theme();
    let r = centered(area, 72, area.height.saturating_sub(4).min(44));
    let inner = frame_box(buf, r, t.text, t.panel, Some(("Dados de uso", t.text)));
    if inner.height < 5 {
        return;
    }
    hline(buf, inner.x, inner.bottom() - 2, inner.width, t.dim.patch(t.panel));
    put(buf, inner.x, inner.bottom() - 1, inner.width, " ↑↓ scroll · esc close", t.dim.patch(t.panel));

    let body = Rect::new(inner.x, inner.y, inner.width, inner.height - 2);
    let width = usize::from(body.width);
    let mut lines = Vec::new();
    for (title, detail) in DETAILS {
        lines.push(Line::from(Span::styled(format!(" {title}"), t.bold)));
        lines.extend(md::wrap(&[Span::styled(detail, t.secondary)], width, &[Span::raw(" ")], &[Span::raw(" ")]));
        lines.push(Line::default());
    }
    let h = usize::from(body.height);
    let max = lines.len().saturating_sub(h);
    s.max_scroll.set(max);
    s.page.set(h);
    for (i, l) in lines.iter().skip(s.scroll.min(max)).take(h).enumerate() {
        put_line(buf, body.x, body.y + u(i), body.width, l);
    }
}
