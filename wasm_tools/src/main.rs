use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: tool_runner <command> [args]");
        eprintln!("Available commands: ls, cat, write, stats");
        std::process::exit(1);
    }

    let command = &args[1];
    let cmd_args = &args[2..];

    match command.as_str() {
        "ls" => list_dir(cmd_args),
        "cat" => read_file(cmd_args),
        "write" => write_file(cmd_args),
        "stats" => text_stats(cmd_args),
        _ => {
            eprintln!("Unknown command: {}", command);
            std::process::exit(1);
        }
    }
}

fn list_dir(args: &[String]) {
    let path = args.first().map(|s| s.as_str()).unwrap_or("/sandbox");
    match fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries {
                if let Ok(entry) = entry {
                    println!("{}", entry.file_name().to_string_lossy());
                }
            }
        }
        Err(e) => eprintln!("Error reading dir {}: {}", path, e),
    }
}

fn read_file(args: &[String]) {
    if let Some(path) = args.first() {
        match fs::read_to_string(path) {
            Ok(content) => print!("{}", content),
            Err(e) => eprintln!("Error reading file {}: {}", path, e),
        }
    } else {
        eprintln!("Usage: cat <file_path>");
    }
}

fn write_file(args: &[String]) {
    if args.len() >= 2 {
        let path = &args[0];
        let content = &args[1..].join(" ");
        match fs::write(path, content) {
            Ok(_) => println!("Successfully wrote to {}", path),
            Err(e) => eprintln!("Error writing to file {}: {}", path, e),
        }
    } else {
        eprintln!("Usage: write <file_path> <content>");
    }
}

fn text_stats(args: &[String]) {
    if let Some(text) = args.first() {
        let chars = text.len();
        let words = text.split_whitespace().count();
        let lines = text.lines().count();
        println!("Chars: {}, Words: {}, Lines: {}", chars, words, lines);
    } else {
        eprintln!("Usage: stats <text>");
    }
}
