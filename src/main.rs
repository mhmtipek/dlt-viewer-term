use std::{
    collections::HashMap,
    fs::{self, File},
    io,
    thread::{self, current},
    time::Duration,
};

use clap::Parser;

use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, KeyCode, KeyEventKind, KeyModifiers},
    layout::Rect,
    text::Text,
};

use dlt_core::{
    dlt::{self, ExtendedHeader, PayloadContent},
    parse::{DltParseError, ParsedMessage},
    read::{DltMessageReader, read_message},
};

use itertools::sorted;

use chrono::Utc;
use chrono::prelude::DateTime;
use std::time::UNIX_EPOCH;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    file: String,
}

// Load file into the data structure of messages
fn load_file(file_name: &String) -> HashMap<u64, dlt_core::dlt::Message> {
    let file = File::open(&file_name).unwrap();
    let mut dlt_reader = DltMessageReader::new(file, true);
    let mut all_messages = HashMap::new();
    let mut parsed_byte_count = 0;

    loop {
        match read_message(&mut dlt_reader, None) {
            Ok(Some(parsed_message)) => match parsed_message {
                ParsedMessage::Item(message) => {
                    parsed_byte_count = parsed_byte_count + message.header.overall_length() as u64;
                    match message.header.timestamp {
                        Some(timestamp) => all_messages.insert(timestamp as u64, message),
                        None => continue,
                    };
                }
                ParsedMessage::FilteredOut(size) => {
                    parsed_byte_count = parsed_byte_count + size as u64;
                    continue;
                }
                ParsedMessage::Invalid => {
                    continue;
                }
            },
            Ok(None) => {
                break;
            }
            Err(error) => match error {
                DltParseError::ParsingHickup(_) => {
                    continue;
                }
                _ => panic!("{}", error),
            },
        }
    }

    all_messages
}

// Takes size in bytes and returns human readible text of size
fn size_to_text(size: u64) -> String {
    if size < 1024 {
        return format!("{} bytes", size);
    }

    if size < (1024 * 1024) {
        return format!("{:.2} KB", size as f32 / 1024.0);
    }

    if size < (1024 * 1024 * 1024) {
        return format!("{:.2} MB", size as f32 / (1024.0 * 1024.0));
    }

    format!("{:.2} GB", size as f32 / (1024.0 * 1024.0 * 1024.0))
}

fn run(mut terminal: DefaultTerminal, file: &String) -> io::Result<()> {
    let file_meta_data = fs::metadata(file)?;

    // Start thread which loads the file
    let file_to_load = file.clone();
    let load_file_thread_handle = thread::spawn(move || load_file(&file_to_load));

    let mut counter = 0;
    let printable_file_name: String = if file.len() > 50 {
        format!(
            "...{}",
            file.chars()
                .skip(file.len() - 50)
                .take(50)
                .collect::<String>()
        )
    } else {
        format!("{file}")
    };
    let printable_file_size = size_to_text(file_meta_data.len());
    loop {
        // Draw loading animation
        let loading_text = format!("Loading file ... ");
        let shift = counter % loading_text.len();
        let print_text = format!("{}{}", &loading_text[shift..], &loading_text[0..shift]);

        terminal.draw(|frame| {
            frame.render_widget(
                Text::from(print_text.clone()).centered(),
                Rect::new(0, (frame.area().height / 2) - 1, frame.area().width, 1),
            );
            frame.render_widget(
                Text::from(format!("{printable_file_name} {printable_file_size}")).right_aligned(),
                Rect::new(0, frame.area().height - 1, frame.area().width, 1),
            );
        })?;

        thread::sleep(Duration::from_millis(100));
        counter = counter + 1;

        if load_file_thread_handle.is_finished() {
            break;
        }
    }

    let messages = load_file_thread_handle.join().unwrap();
    let mut current_index = 0;

    let keys_sorted_by_timestamp = sorted(messages.keys()).collect::<Vec<_>>();

    loop {
        let mut window_height = 0;
        terminal.draw(|frame| {
            window_height = frame.area().height;
            let mut line = 0;
            for i in current_index..keys_sorted_by_timestamp.len() {
                let key = keys_sorted_by_timestamp[i];
                let message = messages[key].clone();
                let ecu_id = message.header.ecu_id.unwrap_or(String::from("????"));
                let extended_header;
                if message.extended_header.is_none() {
                    continue;
                } else {
                    extended_header = message.extended_header.unwrap();
                }
                let app_id = extended_header.application_id;
                let context_id = extended_header.context_id;
                let storage_header;
                if message.storage_header.is_none() {
                    continue;
                } else {
                    storage_header = message.storage_header.unwrap();
                }

                let line_text = format!(
                    "{}.{:<6} {:4} {:4} {:4} {:03} {:<7} {}",
                    DateTime::<Utc>::from(
                        UNIX_EPOCH + Duration::from_secs(storage_header.timestamp.seconds as u64)
                    )
                    .format("%Y/%m/%d %H:%M:%S"),
                    storage_header.timestamp.microseconds,
                    ecu_id,
                    app_id,
                    context_id,
                    message.header.message_counter,
                    message.header.session_id.unwrap_or(0),
                    match message.payload {
                        PayloadContent::Verbose(args) => {
                            let mut message_text: String = String::from("");
                            for arg in args {
                                match arg.value {
                                    dlt::Value::StringVal(text) => {
                                        message_text = text;
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            message_text
                        }
                        PayloadContent::ControlMsg(control_type, data) => {
                            format!(
                                "{:?} {:#X} {:#X} {:#X} {:#X} {:#X} {:#X} {:#X} {:#X}",
                                control_type,
                                data[0],
                                data[1],
                                data[2],
                                data[3],
                                data[4],
                                data[5],
                                data[6],
                                data[7]
                            )
                        }
                        _ => {
                            "".to_string()
                        }
                    }
                );

                frame.render_widget(
                    Text::from(line_text).left_aligned(),
                    Rect::new(0, line, frame.area().width, 1),
                );

                frame.render_widget(
                    Text::from(format!("{printable_file_size} {printable_file_name}"))
                        .right_aligned(),
                    Rect::new(0, frame.area().height - 1, frame.area().width, 1),
                );

                line = line + 1;

                if i >= current_index + frame.area().height as usize - 2 {
                    break;
                }
            }
        })?;

        match event::poll(Duration::from_millis(200)) {
            Ok(true) => {
                if let event::Event::Key(key) = event::read().unwrap() {
                    let is_ctrl_pressed = key.modifiers.intersects(KeyModifiers::CONTROL);
                    match key.kind {
                        KeyEventKind::Press => match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Down => {
                                if current_index
                                    <= keys_sorted_by_timestamp.len() - window_height as usize
                                {
                                    current_index = current_index + 1
                                }
                            }
                            KeyCode::Up => {
                                if current_index > 0 {
                                    current_index = current_index - 1
                                }
                            }
                            KeyCode::Char('u') => {
                                if is_ctrl_pressed {
                                    if current_index > window_height as usize - 1 {
                                        current_index = current_index - window_height as usize + 1;
                                    } else {
                                        current_index = 0;
                                    }
                                }
                            }
                            KeyCode::Char('d') => {
                                if is_ctrl_pressed {
                                    if (current_index + window_height as usize - 1)
                                        < (keys_sorted_by_timestamp.len() - window_height as usize)
                                    {
                                        current_index = current_index + window_height as usize - 1;
                                    } else {
                                        current_index =
                                            keys_sorted_by_timestamp.len() - window_height as usize;
                                    }
                                }
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    let mut terminal = ratatui::init();
    terminal.clear()?;

    let result = run(terminal, &args.file);
    ratatui::restore();

    result
}
