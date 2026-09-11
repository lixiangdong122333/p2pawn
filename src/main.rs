mod app;
mod config;
mod game;
mod input;
mod net;
mod ui;
mod util;

use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};

fn main() -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<()> {
    let mut app = match app::App::new() {
        Ok(a) => a,
        Err(e) => {
            return Err(std::io::Error::other(format!(
                "failed to start networking: {e}"
            )));
        }
    };

    loop {
        app.poll_lan();
        terminal.draw(|f| ui::draw(f, &app))?;
        if app.quit {
            break;
        }
        // Poll for input with a 100ms timeout so the clocks stay fresh.
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            input::handle_key(&mut app, key);
        }
    }

    // Politely close any running LAN game.
    if let Some(sess) = app.session.as_ref()
        && !sess.is_over()
        && let Some(conn) = app.conn.as_mut()
    {
        conn.send(&net::proto::GameMsg::Bye);
    }
    Ok(())
}
