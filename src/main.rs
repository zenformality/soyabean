//! soyabean — a minimal yet powerful terminal code IDE.

mod buffer;
mod draw;
mod editor;
mod finder;
mod syntax;

use std::io;

use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
};
use crossterm::execute;

fn restore_terminal() {
    let mut out = io::stdout();
    let _ = execute!(out, DisableBracketedPaste);
    let _ = execute!(out, DisableMouseCapture, LeaveAlternateScreen, Show);
    let _ = disable_raw_mode();
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("soyabean — a minimal yet powerful terminal code IDE\n");
        println!("usage: soyabean [FILE ...]\n");
        println!("Press F1 inside the editor for keybindings.");
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("soyabean {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let mut ed = editor::Editor::new(&args)?;

    // Always restore the terminal, even on panic.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        hook(info);
    }));

    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture, SetTitle("soyabean"))?;
    // Not supported by every terminal (e.g. legacy Windows console) — ignore.
    let _ = execute!(out, EnableBracketedPaste);

    let res = ed.run(&mut out);

    restore_terminal();
    res
}
