mod app;
mod editor;
mod events;
mod fuzzy;
mod state;
mod ui;

use anyhow::Result;
use clap::Parser;
use crossterm::event::DisableMouseCapture;
use crossterm::event::EnableMouseCapture;
use crossterm::execute;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;

use app::App;
use events::EventHandler;

#[derive(Parser)]
#[command(name = "mmry-tui")]
#[command(about = "TUI for mmry memory management", long_about = None)]
#[command(version)]
struct Cli {
    #[arg(short = 's', long, help = "Store to use (defaults to config default)")]
    store: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Validate store name if provided
    if let Some(ref name) = cli.store {
        mmry_core::stores::validate_store_name(name)?;
    }

    let mut app = App::new(cli.store.as_deref()).await?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {err:?}");
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    let mut event_handler = EventHandler::new();
    let mut needs_full_redraw = false;

    loop {
        if needs_full_redraw {
            terminal.clear()?;
            needs_full_redraw = false;
        }

        terminal.draw(|f| ui::draw(f, app))?;

        if let Some(event) = event_handler.next()? {
            if !app.handle_event(event).await? {
                break;
            }

            // Check if we need a full redraw (after returning from editor)
            if app.needs_redraw {
                needs_full_redraw = true;
                app.needs_redraw = false;
            }
        }
    }

    Ok(())
}
