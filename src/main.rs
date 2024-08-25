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
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Command {
    InsertAfter,
    InsertBefore,
    Replace,
    Delete,
    CreateFile,
    RenameFile,
    DeleteFile,
}

#[derive(Debug, Deserialize)]
struct Change {
    filename: PathBuf,
    new_filename: Option<PathBuf>,
    command: Command,
    reason: String,
    #[serde(default)]
    start_lines: Vec<String>,
    #[serde(default)]
    end_lines: Vec<String>,
    #[serde(default)]
    new_lines: Vec<String>,
}

fn main() -> Result<()> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("Failed to read from stdin")?;

    // Save the stdin data to "llm2fs_last_changes.txt"
    fs::write("llm2fs_last_changes.txt", &input)
        .context("Failed to save stdin data to llm2fs_last_changes.txt")?;

    let response: LLMResponse =
        serde_json::from_str(&input).context("Failed to parse JSON content")?;

    println!("{}\n", response.explanation);

    for (index, change) in response.changes.iter().enumerate() {
        // Check if the filename is within the current directory
        if !is_file_in_current_directory(&change.filename) {
            println!(
                "Warning: Filename '{}' is outside the current directory. Skipping.",
                change.filename.display()
            );
            continue;
        }

        if index > 0 {
            println!();
        }

        println!("=>  File: {}", change.filename.display());
        println!(
            "=>  Action: {}",
            match change.command {
                Command::InsertBefore => "Inserting new content before the specified lines",
                Command::InsertAfter => "Inserting new content after the specified lines",
                Command::Replace => "Replacing existing content",
                Command::Delete => "Deleting content",
                Command::CreateFile => "Creating new file",
                Command::RenameFile => "Renaming file",
                Command::DeleteFile => "Deleting file",
            }
        );
        println!("=>  Reason: {}", change.reason);

        match change.command {
            Command::CreateFile => {
                let file_path = Path::new(&change.filename);
                if file_path.exists() {
                    eprintln!("✗ File already exists: {:?}", change.filename);
                    continue;
                }
                if let Some(parent) = file_path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("✗ Failed to create directory: {:?}", parent))?;
                }
                fs::write(file_path, change.new_lines.join("\n"))
                    .with_context(|| format!("✗ Failed to create file: {:?}", change.filename))?;
                println!("✓ Created file: {:?}", change.filename);
            }
            Command::RenameFile => {
                if let Some(new_filename) = &change.new_filename {
                    fs::rename(&change.filename, new_filename).with_context(|| {
                        format!("✗ Failed to rename file: {:?}", change.filename)
                    })?;
                    println!(
                        "✓ Renamed file: {:?} -> {:?}",
                        change.filename, new_filename
                    );
                } else {
                    eprintln!(
                        "✗ Failed to rename file as no new filename was provided: {:?}",
                        change.filename
                    );
                }
            }
            Command::DeleteFile => {
                fs::remove_file(&change.filename)
                    .with_context(|| format!("✗ Failed to delete file: {:?}", change.filename))?;
                println!("✓ Deleted file: {:?}", change.filename);
            }
            Command::InsertBefore => {
                let file_lines = fs::read_to_string(&change.filename)
                    .with_context(|| format!("✗ Failed to read file: {:?}", change.filename))?
                    .lines()
                    .map(String::from)
                    .collect::<Vec<_>>();

                if let Some(index) = find_in_file_lines(&file_lines, &change.start_lines) {
                    let mut new_lines = file_lines[..index].to_vec();
                    new_lines.extend(change.new_lines.iter().cloned());
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
            Command::InsertAfter => {
                let file_lines = fs::read_to_string(&change.filename)
                    .with_context(|| format!("✗ Failed to read file: {:?}", change.filename))?
                    .lines()
                    .map(String::from)
                    .collect::<Vec<_>>();

                if let Some(index) = find_in_file_lines(&file_lines, &change.start_lines) {
                    let mut new_lines = file_lines[..index].to_vec();
                    new_lines.extend(change.new_lines.iter().cloned());
                    new_lines.extend(file_lines[index..].iter().cloned());
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
            Command::Replace => {
                let file_lines = fs::read_to_string(&change.filename)
                    .with_context(|| format!("✗ Failed to read file: {:?}", change.filename))?
                    .lines()
                    .map(String::from)
                    .collect::<Vec<_>>();

                let start_index = find_in_file_lines(&file_lines, &change.start_lines);
                let end_index = find_in_file_lines(&file_lines, &change.end_lines);

                if let (Some(start_index), Some(end_index)) = (start_index, end_index) {
                    let mut new_lines = file_lines[..start_index].to_vec();
                    new_lines.extend(change.new_lines.iter().cloned());
                    new_lines.extend(
                        file_lines[end_index + change.end_lines.len()..]
                            .iter()
                            .cloned(),
                    );
                    fs::write(&change.filename, new_lines.join("\n")).with_context(|| {
                        format!("✗ Failed to write to file: {:?}", change.filename)
                    })?;
                    println!("✓ Replaced content in file: {:?}", change.filename);
                } else {
                    eprintln!(
                        "✗ Failed to find specified lines in file: {:?}",
                        change.filename
                    );
                }
            }
            Command::Delete => {
                let file_lines = fs::read_to_string(&change.filename)
                    .with_context(|| format!("✗ Failed to read file: {:?}", change.filename))?
                    .lines()
                    .map(String::from)
                    .collect::<Vec<_>>();

                let start_index = find_in_file_lines(&file_lines, &change.start_lines);
                let end_index = find_in_file_lines(&file_lines, &change.end_lines);

                if let (Some(start_index), Some(end_index)) = (start_index, end_index) {
                    let mut new_lines = file_lines[..start_index].to_vec();
                    new_lines.extend(
                        file_lines[end_index + change.end_lines.len()..]
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

    println!("\n{}", response.conclusion);

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
