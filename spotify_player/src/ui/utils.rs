use super::{
    config, Alignment, Block, Constraint, Frame, Layout, Line, List, ListItem, ListState, Modifier,
    Paragraph, Rect, Span, Style, Table, TableState,
};
use unicode_bidi::BidiInfo;

const SHELL_WIDE_THRESHOLD: u16 = 120;
const SHELL_MAX_WIDTH: u16 = 140;
const SHELL_MIN_SIDE_PADDING: u16 = 2;
const SHELL_TEXT_INSET: u16 = 2;
const ROW_HIGHLIGHT_SYMBOL: &str = "| ";

pub fn content_shell_rect(rect: Rect) -> Rect {
    if rect.width < SHELL_WIDE_THRESHOLD {
        return rect;
    }

    let bounded_width = rect.width.min(SHELL_MAX_WIDTH);
    let max_width_with_padding = rect
        .width
        .saturating_sub(SHELL_MIN_SIDE_PADDING.saturating_mul(2));
    let width = bounded_width.min(max_width_with_padding).max(1);
    let x = rect.x + (rect.width.saturating_sub(width)) / 2;

    Rect::new(x, rect.y, width, rect.height)
}

pub fn inset_rect(rect: Rect, inset: u16) -> Rect {
    Rect::new(
        rect.x.saturating_add(inset),
        rect.y,
        rect.width.saturating_sub(inset),
        rect.height,
    )
}

pub fn shell_text_rect(rect: Rect) -> Rect {
    inset_rect(rect, SHELL_TEXT_INSET)
}

pub fn app_badge_rect(rect: Rect, badge_width: u16) -> Rect {
    let text_rect = shell_text_rect(rect);
    Rect::new(
        text_rect.x,
        text_rect.y,
        badge_width.min(text_rect.width),
        text_rect.height,
    )
}

fn normalize_gutter_selection_style(style: Style) -> Style {
    let mut style = style.remove_modifier(Modifier::REVERSED);
    style.bg = None;
    style
}

pub fn gutter_selection_style(theme: &config::Theme, is_active: bool) -> Style {
    if !is_active {
        return Style::default();
    }

    normalize_gutter_selection_style(theme.selection(true))
}

pub fn panel_style(theme: &config::Theme) -> Style {
    theme.app()
}

pub fn render_panel<'a>(
    frame: &mut Frame,
    theme: &config::Theme,
    rect: Rect,
    title: &str,
    meta: Option<Line<'a>>,
    is_active: bool,
) -> Rect {
    frame.render_widget(Block::default().style(panel_style(theme)), rect);

    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Fill(0)]).split(rect);
    render_section_header(frame, theme, chunks[0], title, meta, is_active);
    chunks[1]
}

pub fn render_section_header<'a>(
    frame: &mut Frame,
    theme: &config::Theme,
    rect: Rect,
    title: &str,
    meta: Option<Line<'a>>,
    is_active: bool,
) {
    let rect = shell_text_rect(rect);
    let title_style = if is_active {
        theme.page_desc()
    } else {
        theme.playback_metadata()
    };
    if title.is_empty() {
        if let Some(meta) = meta {
            frame.render_widget(Paragraph::new(meta).alignment(Alignment::Right), rect);
        }
    } else {
        let title_width = title.len() as u16 + 2;
        let chunks =
            Layout::horizontal([Constraint::Length(title_width), Constraint::Fill(0)]).split(rect);
        frame.render_widget(
            Paragraph::new(Span::styled(title.to_lowercase(), title_style)),
            chunks[0],
        );
        if let Some(meta) = meta {
            frame.render_widget(Paragraph::new(meta).alignment(Alignment::Right), chunks[1]);
        }
    }
}

/// Construct a generic list widget
pub fn construct_list_widget<'a>(
    theme: &config::Theme,
    items: Vec<(String, bool)>,
    is_active: bool,
) -> (List<'a>, usize) {
    let n_items = items.len();

    (
        List::new(
            items
                .into_iter()
                .map(|(s, is_active)| {
                    ListItem::new(s).style(if is_active {
                        theme.current_playing()
                    } else {
                        Style::default()
                    })
                })
                .collect::<Vec<_>>(),
        )
        .highlight_symbol(highlight_symbol(theme, is_active))
        .highlight_style(gutter_selection_style(theme, is_active)),
        n_items,
    )
}

/// adjust the `selected` position of a `ListState` if that position is invalid
fn adjust_list_state(state: &mut ListState, len: usize) {
    if let Some(p) = state.selected() {
        if p >= len {
            state.select(if len > 0 { Some(len - 1) } else { Some(0) });
        }
    } else if len > 0 {
        state.select(Some(0));
    }
}

pub fn render_list_window(
    frame: &mut Frame,
    widget: List,
    rect: Rect,
    len: usize,
    state: &mut ListState,
) {
    adjust_list_state(state, len);
    frame.render_stateful_widget(widget, rect, state);
}

pub fn render_table_window_from_list_state(
    frame: &mut Frame,
    widget: Table,
    rect: Rect,
    len: usize,
    state: &mut ListState,
) {
    adjust_list_state(state, len);

    let mut table_state = TableState::default();
    table_state.select(state.selected());
    frame.render_stateful_widget(widget, rect, &mut table_state);
}

/// adjust the `selected` position of a `TableState` if that position is invalid
fn adjust_table_state(state: &mut TableState, len: usize) {
    if let Some(p) = state.selected() {
        if p >= len {
            state.select(if len > 0 { Some(len - 1) } else { Some(0) });
        }
    } else if len > 0 {
        state.select(Some(0));
    }
}

pub fn render_table_window(
    frame: &mut Frame,
    widget: Table,
    rect: Rect,
    len: usize,
    state: &mut TableState,
) {
    adjust_table_state(state, len);
    frame.render_stateful_widget(widget, rect, state);
}

pub fn highlight_symbol(theme: &config::Theme, is_active: bool) -> Line<'static> {
    let symbol_style = if is_active {
        theme.playback_status()
    } else {
        Style::default()
    };
    Line::from(vec![Span::styled(ROW_HIGHLIGHT_SYMBOL, symbol_style)])
}

/// Convert a string to a bidirectional string.
/// Used to handle RTL text properly in the UI.
pub fn to_bidi_string(s: &str) -> String {
    let bidi_info = BidiInfo::new(s, None);

    let bidi_string = if bidi_info.has_rtl() && !bidi_info.paragraphs.is_empty() {
        bidi_info
            .reorder_line(&bidi_info.paragraphs[0], 0..s.len())
            .into_owned()
    } else {
        s.to_string()
    };

    bidi_string
}

/// formats genres depending on the number of genres and `genre_num`
///
/// Examples for `genre_num = 2`
/// - 1 genre: "genre1"
/// - 2 genres: "genre1, genre2"
/// - \>= 3 genres: "genre1, genre2, ..."
pub fn format_genres(genres: &[String], genre_num: u8) -> String {
    let mut genre_str = String::with_capacity(64);

    if genre_num > 0 {
        for i in 0..genres.len() {
            genre_str.push_str(&genres[i]);

            if i + 1 != genres.len() {
                genre_str.push_str(", ");

                if i + 1 >= genre_num as usize {
                    genre_str.push_str("...");
                    break;
                }
            }
        }
    }

    genre_str
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_stays_full_width_on_mid_size_terminal() {
        let rect = Rect::new(0, 0, 100, 30);
        assert_eq!(content_shell_rect(rect), rect);
    }

    #[test]
    fn shell_is_centered_and_bounded_on_wide_terminal() {
        let rect = Rect::new(0, 0, 180, 40);
        let shell = content_shell_rect(rect);

        assert_eq!(shell.width, SHELL_MAX_WIDTH);
        assert_eq!(shell.x, 20);
    }

    #[test]
    fn shell_preserves_side_padding_even_when_max_width_is_not_hit() {
        let rect = Rect::new(0, 0, 122, 35);
        let shell = content_shell_rect(rect);

        assert_eq!(shell.width, 118);
        assert_eq!(shell.x, 2);
    }

    #[test]
    fn shell_fixture_matrix_matches_phase_zero_expectations() {
        let cases = [
            (100, 32, 100, 0),
            (120, 36, 116, 2),
            (140, 40, 136, 2),
            (180, 48, 140, 20),
        ];

        for (width, height, expected_width, expected_x) in cases {
            let shell = content_shell_rect(Rect::new(0, 0, width, height));

            assert_eq!(
                shell.width, expected_width,
                "unexpected shell width for {width}x{height}"
            );
            assert_eq!(
                shell.x, expected_x,
                "unexpected shell x for {width}x{height}"
            );
            assert_eq!(
                shell.height, height,
                "unexpected shell height for {width}x{height}"
            );
        }
    }

    #[test]
    fn app_chrome_alignment_matches_phase_zero_shell_matrix() {
        let badge_width = " jx-spotify ".chars().count() as u16;
        let cases = [(100, 32, 2), (120, 36, 4), (140, 40, 4), (180, 48, 22)];

        for (width, height, expected_text_x) in cases {
            let shell = content_shell_rect(Rect::new(0, 0, width, height));
            let text_rect = shell_text_rect(shell);
            let badge_rect = app_badge_rect(shell, badge_width);

            assert_eq!(
                text_rect.x, expected_text_x,
                "unexpected section text inset for {width}x{height}"
            );
            assert_eq!(
                badge_rect.x, expected_text_x,
                "unexpected badge inset for {width}x{height}"
            );
            assert_eq!(
                text_rect.width,
                shell.width.saturating_sub(SHELL_TEXT_INSET),
                "unexpected text width for {width}x{height}"
            );
            assert_eq!(
                badge_rect.width, badge_width,
                "unexpected badge width for {width}x{height}"
            );
        }
    }

    #[test]
    fn gutter_selection_style_drops_background_fill() {
        let selection = Style::default()
            .fg(ratatui::style::Color::White)
            .bg(ratatui::style::Color::Black)
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::REVERSED);
        let normalized = normalize_gutter_selection_style(selection);

        assert_eq!(normalized.bg, None);
        assert!(!normalized.add_modifier.contains(Modifier::REVERSED));
        assert!(normalized.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn highlight_symbol_uses_shared_rail_with_trailing_gap() {
        let active = highlight_symbol(&config::Theme::default(), true);
        let rendered = active
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(rendered, ROW_HIGHLIGHT_SYMBOL);
    }
}
