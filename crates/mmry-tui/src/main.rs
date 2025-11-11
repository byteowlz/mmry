mod app;
mod editor;
mod events;
mod state;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

use app::App;
use events::EventHandler;

#[tokio::main]
async fn main() -> Result<()> {
    let mut app = App::new().await?;

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
