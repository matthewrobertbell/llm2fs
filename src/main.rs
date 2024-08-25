use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct LLMResponse {
    explanation: String,
    changes: Vec<Change>,
    conclusion: String,
}

#[derive(Debug, Deserialize, PartialEq)]
enum LineOrLines {
    Line(String),
    Lines(Vec<String>),
}

impl LineOrLines {
    fn lines(&self) -> Vec<String> {
        match self {
            LineOrLines::Line(line) => vec![line.clone()],
            LineOrLines::Lines(lines) => lines.clone(),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Command {
    InsertAfter {
        insert_lines: LineOrLines,
        marker_lines: LineOrLines,
    },
    InsertBefore {
        insert_lines: LineOrLines,
        marker_lines: LineOrLines,
    },
    Delete {
        delete_lines: LineOrLines,
    },
    CreateFile {
        new_lines: LineOrLines,
    },
    RenameFile {
        new_filename: PathBuf,
    },
    DeleteFile,
}

#[derive(Debug, Deserialize)]
struct Change {
    filename: PathBuf,
    command: Command,
    reason: String,
}

fn main() -> Result<()> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("Failed to read from stdin")?;

    let input = input
        .split_once("{")
        .map(|(_, v)| "{".to_string() + v)
        .unwrap_or(input);

    // Save the stdin data to a file in the llm2fs_changes directory
    let changes_dir = Path::new("llm2fs_changes");
    fs::create_dir_all(changes_dir).context("Failed to create llm2fs_changes directory")?;

    let timestamp = chrono::Local::now().format("%Y-%m-%d-%H-%M-%S").to_string();
    let filename = format!("{}.txt", timestamp);
    let file_path = changes_dir.join(filename);

    fs::write(&file_path, &input)
        .with_context(|| format!("Failed to save stdin data to {:?}", file_path))?;

    let response: LLMResponse =
        serde_json::from_str(&input).context("Failed to parse JSON content")?;

    println!("{}\n------", response.explanation);

    for change in &response.changes {
        if !is_file_in_current_directory(&change.filename) {
            println!(
                "Warning: Filename '{}' is outside the current directory. Skipping.",
                change.filename.display()
            );
            continue;
        }

        println!();

        println!("=>  File: {}", change.filename.display());
        println!(
            "=>  Action: {}",
            match change.command {
                Command::InsertBefore { .. } => "Inserting new content before the specified lines",
                Command::InsertAfter { .. } => "Inserting new content after the specified lines",
                Command::Delete { .. } => "Deleting content",
                Command::CreateFile { .. } => "Creating new file",
                Command::RenameFile { .. } => "Renaming file",
                Command::DeleteFile => "Deleting file",
            }
        );
        println!("=>  Reason: {}", change.reason);

        match &change.command {
            Command::CreateFile { new_lines } => {
                let file_path = Path::new(&change.filename);
                if file_path.exists() {
                    eprintln!("✗ File already exists: {:?}", change.filename);
                    continue;
                }
                if let Some(parent) = file_path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("✗ Failed to create directory: {:?}", parent))?;
                }
                fs::write(file_path, new_lines.lines().join("\n"))
                    .with_context(|| format!("✗ Failed to create file: {:?}", change.filename))?;
                println!("✓ Created file: {:?}", change.filename);
            }
            Command::RenameFile { new_filename } => {
                fs::rename(&change.filename, new_filename)
                    .with_context(|| format!("✗ Failed to rename file: {:?}", change.filename))?;
                println!(
                    "✓ Renamed file: {:?} -> {:?}",
                    change.filename, new_filename
                );
            }
            Command::DeleteFile => {
                fs::remove_file(&change.filename)
                    .with_context(|| format!("✗ Failed to delete file: {:?}", change.filename))?;
                println!("✓ Deleted file: {:?}", change.filename);
            }
            Command::InsertBefore {
                insert_lines,
                marker_lines,
            } => {
                let file_lines = fs::read_to_string(&change.filename)
                    .with_context(|| format!("✗ Failed to read file: {:?}", change.filename))?
                    .lines()
                    .map(String::from)
                    .collect::<Vec<_>>();

                if let Some(index) = find_in_file_lines(&file_lines, &marker_lines.lines()) {
                    let mut new_lines = file_lines[..index].to_vec();
                    new_lines.extend(insert_lines.lines().iter().cloned());
                    new_lines.extend(file_lines[index..].iter().cloned());
                    fs::write(&change.filename, new_lines.join("\n")).with_context(|| {
                        format!("✗ Failed to write to file: {:?}", change.filename)
                    })?;
                    println!(
                        "✓ Inserted new content before specified lines in file: {:?}",
                        change.filename
                    );
                } else {
                    eprintln!(
                        "✗ Failed to find specified lines in file: {:?}",
                        change.filename
                    );
                }
            }
            Command::InsertAfter {
                marker_lines,
                insert_lines,
            } => {
                let file_lines = fs::read_to_string(&change.filename)
                    .with_context(|| format!("✗ Failed to read file: {:?}", change.filename))?
                    .lines()
                    .map(String::from)
                    .collect::<Vec<_>>();

                if let Some(index) = find_in_file_lines(&file_lines, &marker_lines.lines()) {
                    let mut new_lines =
                        file_lines[..=index + marker_lines.lines().len() - 1].to_vec();
                    new_lines.extend(insert_lines.lines().iter().cloned());
                    new_lines.extend(
                        file_lines[index + marker_lines.lines().len()..]
                            .iter()
                            .cloned(),
                    );
                    fs::write(&change.filename, new_lines.join("\n")).with_context(|| {
                        format!("✗ Failed to write to file: {:?}", change.filename)
                    })?;
                    println!(
                        "✓ Inserted new content after specified lines in file: {:?}",
                        change.filename
                    );
                } else {
                    eprintln!(
                        "✗ Failed to find specified lines in file: {:?}",
                        change.filename
                    );
                }
            }
            Command::Delete { delete_lines } => {
                let file_lines = fs::read_to_string(&change.filename)
                    .with_context(|| format!("✗ Failed to read file: {:?}", change.filename))?
                    .lines()
                    .map(String::from)
                    .collect::<Vec<_>>();

                if let Some(start_index) = find_in_file_lines(&file_lines, &delete_lines.lines()) {
                    let mut new_lines = file_lines[..start_index].to_vec();
                    new_lines.extend(
                        file_lines[start_index + delete_lines.lines().len()..]
                            .iter()
                            .cloned(),
                    );
                    fs::write(&change.filename, new_lines.join("\n")).with_context(|| {
                        format!("✗ Failed to write to file: {:?}", change.filename)
                    })?;
                    println!("✓ Deleted content in file: {:?}", change.filename);
                } else {
                    eprintln!(
                        "✗ Failed to find specified lines to delete in file: {:?}",
                        change.filename
                    );
                }
            }
        }
    }

    println!("\n------\n{}", response.conclusion);

    Ok(())
}

fn is_file_in_current_directory(filename: &Path) -> bool {
    let path = Path::new(filename);
    path.is_relative() && !path.starts_with("..")
}

fn find_in_file_lines(file_lines: &[String], needle: &[String]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > file_lines.len() {
        return None;
    }

    for (i, window) in file_lines.windows(needle.len()).enumerate() {
        if window == needle {
            return Some(i);
        }
    }

    None
}
