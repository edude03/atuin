use std::{
    io::{stdout, Write},
    time::Duration,
};

use crossterm::{
    event::{self, KeyCode, KeyEvent, KeyModifiers},
    execute, terminal,
};
use eyre::Result;
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans, Text},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame, Terminal,
};
use unicode_width::UnicodeWidthStr;

use atuin_client::{
    database::Context,
    database::Database,
    history::History,
    settings::{FilterMode, SearchMode},
};

use super::{
    cursor::Cursor,
    history_list::{HistoryList, ListState, PREFIX_LENGTH},
};
use crate::VERSION;

struct State {
    history_count: i64,
    input: Cursor,
    filter_mode: FilterMode,
    results_state: ListState,
    context: Context,
}

impl State {
    fn query_results(
        &mut self,
        search_mode: SearchMode,
        db: &mut impl Database,
    ) -> Result<Vec<History>> {
        let i = self.input.as_str();
        let results = if i.is_empty() {
            db.list(self.filter_mode, &self.context, Some(200), true)?
        } else {
            db.search(Some(200), search_mode, self.filter_mode, &self.context, i)?
        };

        self.results_state.select(0);
        Ok(results)
    }

    fn handle_input(&mut self, input: &KeyEvent, len: usize) -> Option<usize> {
        let ctrl = input.modifiers.contains(KeyModifiers::CONTROL);
        match input.code {
            KeyCode::Esc => return Some(usize::MAX),
            KeyCode::Char('c' | 'd' | 'g') if ctrl => return Some(usize::MAX),
            KeyCode::Enter => {
                return Some(self.results_state.selected());
            }
            KeyCode::Char(c @ '1'..='9') if input.modifiers.contains(KeyModifiers::ALT) => {
                let c = c.to_digit(10)? as usize;
                return Some(self.results_state.selected() + c);
            }
            KeyCode::Char('h') if ctrl => {
                self.input.left();
            }
            KeyCode::Left => {
                self.input.left();
            }
            KeyCode::Char('l') if ctrl => self.input.right(),
            KeyCode::Right => self.input.right(),
            KeyCode::Char('a') if ctrl => self.input.start(),
            KeyCode::Char('e') if ctrl => self.input.end(),
            KeyCode::Backspace => {
                self.input.back();
            }
            KeyCode::Char('w') if ctrl => {
                // remove the first batch of whitespace
                while matches!(self.input.back(), Some(c) if c.is_whitespace()) {}
                while self.input.left() {
                    if self.input.char().unwrap().is_whitespace() {
                        self.input.right(); // found whitespace, go back right
                        break;
                    }
                    self.input.remove();
                }
            }
            KeyCode::Char('u') if ctrl => self.input.clear(),
            KeyCode::Char('r') if ctrl => {
                pub static FILTER_MODES: [FilterMode; 4] = [
                    FilterMode::Global,
                    FilterMode::Host,
                    FilterMode::Session,
                    FilterMode::Directory,
                ];
                let i = self.filter_mode as usize;
                let i = (i + 1) % FILTER_MODES.len();
                self.filter_mode = FILTER_MODES[i];
            }
            KeyCode::Down => {
                let i = self.results_state.selected().saturating_sub(1);
                self.results_state.select(i);
            }
            KeyCode::Char('n' | 'j') if ctrl => {
                let i = self.results_state.selected().saturating_sub(1);
                self.results_state.select(i);
            }
            KeyCode::Up => {
                let i = self.results_state.selected() + 1;
                self.results_state.select(i.min(len - 1));
            }
            KeyCode::Char('p' | 'k') if ctrl => {
                let i = self.results_state.selected() + 1;
                self.results_state.select(i.min(len - 1));
            }
            KeyCode::Char(c) => self.input.insert(c),
            _ => {}
        };

        None
    }

    #[allow(clippy::cast_possible_truncation)]
    fn draw<T: Backend>(&mut self, f: &mut Frame<'_, T>, results: &[History]) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(0)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(3),
            ])
            .split(f.size());

        let top_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50); 2])
            .split(chunks[0]);

        let top_left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1); 3])
            .split(top_chunks[0]);

        let top_right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1); 3])
            .split(top_chunks[1]);

        let title = Paragraph::new(Text::from(Span::styled(
            format!(" Atuin v{VERSION}"),
            Style::default().add_modifier(Modifier::BOLD),
        )));

        let help = vec![
            Span::raw(" Press "),
            Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" to exit."),
        ];

        let help = Paragraph::new(Text::from(Spans::from(help)));
        let stats = Paragraph::new(Text::from(Span::raw(format!(
            "history count: {} ",
            self.history_count
        ))));

        f.render_widget(title, top_left_chunks[1]);
        f.render_widget(help, top_left_chunks[2]);
        f.render_widget(stats.alignment(Alignment::Right), top_right_chunks[1]);

        let results = HistoryList::new(results).block(
            Block::default()
                .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                .border_type(BorderType::Rounded),
        );

        f.render_stateful_widget(results, chunks[1], &mut self.results_state);

        let input = format!(
            "[{:^14}] {}",
            self.filter_mode.as_str(),
            self.input.as_str(),
        );
        let input = Paragraph::new(input).block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_type(BorderType::Rounded)
                .title(format!(
                    "{:─>width$}",
                    "",
                    width = chunks[2].width as usize - 2
                )),
        );
        f.render_widget(input, chunks[2]);

        let width = UnicodeWidthStr::width(self.input.substring());
        f.set_cursor(
            // Put cursor past the end of the input text
            chunks[2].x + width as u16 + PREFIX_LENGTH + 2,
            // Move one line down, from the border to the input line
            chunks[2].y + 1,
        );
    }

    #[allow(clippy::cast_possible_truncation)]
    fn draw_compact<T: Backend>(&mut self, f: &mut Frame<'_, T>, results: &[History]) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(0)
            .horizontal_margin(1)
            .constraints(
                [
                    Constraint::Length(1),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ]
                .as_ref(),
            )
            .split(f.size());

        let header_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Ratio(1, 3),
                    Constraint::Ratio(1, 3),
                    Constraint::Ratio(1, 3),
                ]
                .as_ref(),
            )
            .split(chunks[0]);

        let title = Paragraph::new(Text::from(Span::styled(
            format!("Atuin v{}", VERSION),
            Style::default().fg(Color::DarkGray),
        )));

        let help = Paragraph::new(Text::from(Spans::from(vec![
            Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" to exit"),
        ])))
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);

        let stats = Paragraph::new(Text::from(Span::raw(format!(
            "history count: {}",
            self.history_count,
        ))))
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Right);

        f.render_widget(title, header_chunks[0]);
        f.render_widget(help, header_chunks[1]);
        f.render_widget(stats, header_chunks[2]);

        let results = HistoryList::new(results);
        f.render_stateful_widget(results, chunks[1], &mut self.results_state);

        let input = format!(
            "[{:^14}] {}",
            self.filter_mode.as_str(),
            self.input.as_str(),
        );
        let input = Paragraph::new(input);
        f.render_widget(input, chunks[2]);

        let extra_width = UnicodeWidthStr::width(self.input.substring());

        f.set_cursor(
            // Put cursor past the end of the input text
            chunks[2].x + extra_width as u16 + PREFIX_LENGTH + 1,
            // Move one line down, from the border to the input line
            chunks[2].y + 1,
        );
    }
}

struct Stdout {
    stdout: std::io::Stdout,
}

impl Stdout {
    pub fn new() -> std::io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(
            stdout,
            terminal::EnterAlternateScreen,
            event::EnableMouseCapture
        )?;
        Ok(Self { stdout })
    }
}

impl Drop for Stdout {
    fn drop(&mut self) {
        execute!(
            self.stdout,
            terminal::LeaveAlternateScreen,
            event::DisableMouseCapture
        )
        .unwrap();
        terminal::disable_raw_mode().unwrap();
    }
}

impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.stdout.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stdout.flush()
    }
}

// this is a big blob of horrible! clean it up!
// for now, it works. But it'd be great if it were more easily readable, and
// modular. I'd like to add some more stats and stuff at some point
#[allow(clippy::cast_possible_truncation)]
pub fn history(
    query: &[String],
    search_mode: SearchMode,
    filter_mode: FilterMode,
    style: atuin_client::settings::Style,
    db: &mut impl Database,
) -> Result<String> {
    let stdout = Stdout::new()?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut input = Cursor::from(query.join(" "));
    // Put the cursor at the end of the query by default
    input.end();
    let mut app = State {
        history_count: db.history_count()?,
        input,
        results_state: ListState::default(),
        context: Context::default(),
        filter_mode,
    };

    let mut results = app.query_results(search_mode, db)?;

    let index = 'render: loop {
        let initial_input = app.input.as_str().to_owned();
        let initial_filter_mode = app.filter_mode;

        if event::poll(Duration::from_millis(250))? {
            while event::poll(Duration::ZERO)? {
                if let event::Event::Key(input) = event::read()? {
                    if let Some(i) = app.handle_input(&input, results.len()) {
                        break 'render i;
                    }
                }
            }
        }

        if initial_input != app.input.as_str() || initial_filter_mode != app.filter_mode {
            results = app.query_results(search_mode, db)?;
        }

        let compact = match style {
            atuin_client::settings::Style::Auto => {
                terminal.size().map(|size| size.height < 14).unwrap_or(true)
            }
            atuin_client::settings::Style::Compact => true,
            atuin_client::settings::Style::Full => false,
        };
        if compact {
            terminal.draw(|f| app.draw_compact(f, &results))?;
        } else {
            terminal.draw(|f| app.draw(f, &results))?;
        }
    };

    if index < results.len() {
        // index is in bounds so we return that entry
        Ok(results.swap_remove(index).command)
    } else if index == usize::MAX {
        // index is max which implies an early exit
        Ok(String::new())
    } else {
        // out of bounds usually implies no selected entry so we return the input
        Ok(app.input.into_inner())
    }
}
