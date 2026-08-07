use ratatui::text::{Line, Span};

use crate::app::App;
use crate::ui::text;
use crate::ui::theme;

/// How a column claims horizontal space. Fixed columns take exactly what they
/// ask for; the rest share what is left, in proportion.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Width {
    Fixed(usize),
    Fill(usize),
}

#[derive(Debug, Clone)]
pub struct Column {
    pub header: String,
    pub width: Width,
}

impl Column {
    pub fn fixed(header: impl Into<String>, width: usize) -> Self {
        Self {
            header: header.into(),
            width: Width::Fixed(width),
        }
    }

    pub fn fill(header: impl Into<String>, weight: usize) -> Self {
        Self {
            header: header.into(),
            width: Width::Fill(weight),
        }
    }
}

/// A cell keeps its own role only when it means something the row does not —
/// a liveness glyph, say. Otherwise it inherits the row.
#[derive(Debug, Clone)]
pub struct Cell {
    pub text: String,
    pub role: Option<theme::StyleRole>,
}

impl Cell {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            role: None,
        }
    }

    pub const fn with_role(mut self, role: theme::StyleRole) -> Self {
        self.role = Some(role);
        self
    }
}

impl From<&str> for Cell {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Cell {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub role: theme::StyleRole,
    pub selected: bool,
}

impl Row {
    pub fn new(cells: impl IntoIterator<Item = impl Into<Cell>>) -> Self {
        Self {
            cells: cells.into_iter().map(Into::into).collect(),
            role: theme::StyleRole::TextPrimary,
            selected: false,
        }
    }

    pub const fn with_role(mut self, role: theme::StyleRole) -> Self {
        self.role = role;
        self
    }

    pub const fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

/// Two, not one. A column sized to its own heading fills its cell exactly, and
/// a single space then reads as a word break rather than a column break:
/// `EXPOSURE LISTENER`. Wider columns hide the problem behind their own slack,
/// which is why it only ever shows up in one place at a time.
const GAP: usize = 2;

/// Every list in Tale is this shape: a heading row, then one line per row, the
/// selection carrying the row style. Views differ in their columns, never in
/// how a list looks.
pub fn lines(app: &App, columns: &[Column], rows: &[Row], width: u16) -> Vec<Line<'static>> {
    let widths = resolve(columns, width);
    let mut lines = vec![Line::from(row_spans(
        app,
        columns
            .iter()
            .map(|column| Cell::new(column.header.clone()))
            .collect(),
        &widths,
        theme::StyleRole::TextPrimary,
        false,
    ))];
    for row in rows {
        lines.push(Line::from(row_spans(
            app,
            row.cells.clone(),
            &widths,
            row.role,
            row.selected,
        )));
    }
    lines
}

/// A selected row is one style end to end, so the highlight reads as a bar
/// rather than as a run of differently coloured words.
fn row_spans(
    app: &App,
    cells: Vec<Cell>,
    widths: &[usize],
    row_role: theme::StyleRole,
    selected: bool,
) -> Vec<Span<'static>> {
    let last = widths.len().saturating_sub(1);
    cells
        .into_iter()
        .enumerate()
        .map(|(index, cell)| {
            let padded = match widths.get(index) {
                Some(width) => text::pad_or_trim(&cell.text, *width),
                None => cell.text.clone(),
            };
            let trailing = if index >= last {
                String::new()
            } else {
                " ".repeat(GAP)
            };
            Span::styled(
                format!("{padded}{trailing}"),
                if selected {
                    app.theme.style(theme::StyleRole::Selection)
                } else {
                    app.theme.style(cell.role.unwrap_or(row_role))
                },
            )
        })
        .collect()
}

/// Fixed columns are honoured first; whatever is left is split by weight, so a
/// narrow terminal shrinks the flexible columns rather than dropping any.
fn resolve(columns: &[Column], width: u16) -> Vec<usize> {
    let separators = columns.len().saturating_sub(1).saturating_mul(GAP);
    let available = usize::from(width).saturating_sub(separators);
    let fixed: usize = columns
        .iter()
        .filter_map(|column| match column.width {
            Width::Fixed(value) => Some(value),
            Width::Fill(_) => None,
        })
        .sum();
    let weight: usize = columns
        .iter()
        .filter_map(|column| match column.width {
            Width::Fill(value) => Some(value),
            Width::Fixed(_) => None,
        })
        .sum();
    let flexible = available.saturating_sub(fixed);
    let mut widths = Vec::with_capacity(columns.len());
    let mut spent = 0;
    let mut remaining_weight = weight;
    for column in columns {
        match column.width {
            Width::Fixed(value) => widths.push(value),
            Width::Fill(value) => {
                // The last flexible column absorbs the rounding remainder so no
                // row ends a character short of the border.
                let share = if remaining_weight == value {
                    flexible.saturating_sub(spent)
                } else {
                    flexible
                        .saturating_mul(value)
                        .checked_div(weight)
                        .unwrap_or(0)
                };
                spent = spent.saturating_add(share);
                remaining_weight = remaining_weight.saturating_sub(value);
                widths.push(share.max(3));
            }
        }
    }
    widths
}

/// The other shape Tale repeats: a label column and a value. Used wherever one
/// thing is described rather than many things listed.
pub fn detail(app: &App, pairs: &[(&str, String)]) -> Vec<Line<'static>> {
    let label_width = pairs
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0)
        .saturating_add(2);
    pairs
        .iter()
        .map(|(label, value)| {
            Line::from(vec![
                Span::styled(
                    text::pad_or_trim(label, label_width),
                    app.theme.style(theme::StyleRole::TextMuted),
                ),
                Span::styled(
                    value.clone(),
                    app.theme.style(theme::StyleRole::TextPrimary),
                ),
            ])
        })
        .collect()
}
