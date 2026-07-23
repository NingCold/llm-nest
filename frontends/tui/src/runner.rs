use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind, MouseEventKind};
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::{App, InputEvent, UserEvent};
use crate::event::handle_event;
use crate::ui;

pub async fn run(mut app: App) -> Result<()> {
    let mut terminal = ratatui::init();
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;

    let res = run_loop(&mut terminal, &mut app).await;

    crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)?;
    ratatui::restore();
    res
}

async fn run_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    let (evt_tx, mut evt_rx) = mpsc::unbounded_channel::<UserEvent>();
    let cancel = CancellationToken::new();

    let input_tx = evt_tx.clone();
    let cancel_clone = cancel.clone();
    tokio::task::spawn_blocking(move || {
        loop {
            if cancel_clone.is_cancelled() {
                break;
            }
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                match event::read() {
                    Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                        if input_tx
                            .send(UserEvent::Input(InputEvent::Key(key)))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(Event::Mouse(mouse)) => {
                        let ievt = match mouse.kind {
                            MouseEventKind::ScrollDown => Some(InputEvent::ScrollDown),
                            MouseEventKind::ScrollUp => Some(InputEvent::ScrollUp),
                            _ => None,
                        };
                        if let Some(ievt) = ievt
                            && input_tx.send(UserEvent::Input(ievt)).is_err()
                        {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    let frame_interval = Duration::from_secs_f64(1.0 / 60.0);
    let mut next_frame = Instant::now();
    let mut redraw_timer = tokio::time::interval(Duration::from_millis(16));
    redraw_timer.tick().await;

    loop {
        if Instant::now() >= next_frame && app.dirty {
            let draw_start = Instant::now();
            terminal.draw(|f| ui::ui(f, app))?;
            let draw_elapsed = draw_start.elapsed();

            if draw_elapsed > Duration::from_millis(5) {
                tracing::warn!(
                    target: "tui::perf",
                    elapsed_ms = draw_elapsed.as_secs_f64() * 1000.0,
                    "slow frame",
                );
            } else if draw_elapsed > Duration::from_millis(2) {
                tracing::debug!(
                    target: "tui::perf",
                    elapsed_ms = draw_elapsed.as_secs_f64() * 1000.0,
                    "frame",
                );
            }

            app.dirty = false;
            app.draw_count += 1;
            next_frame = Instant::now() + frame_interval;
        }

        if app.waiting {
            app.thinking_tick += 1;
        }

        tokio::select! {
            biased;
            Some(uevt) = evt_rx.recv() => {
                handle_event(app, uevt, &evt_tx);
            }
            _ = redraw_timer.tick() => {
                if app.waiting {
                    app.mark_dirty();
                }
            }
        }

        if app.should_quit {
            cancel.cancel();
            return Ok(());
        }
    }
}
