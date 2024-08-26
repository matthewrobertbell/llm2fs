use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct LLMResponse {
    explanation: String,
    changes: Vec<Change>,
    conclusion: String,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
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

    fn len(&self) -> usize {
        match self {
            LineOrLines::Line(_) => 1,
            LineOrLines::Lines(lines) => lines.len(),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "command", rename_all = "SCREAMING_SNAKE_CASE")]
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
    #[serde(flatten)]
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

        println!("=> File: {}", change.filename.display());
        println!(
            "=> Action: {}",
            match change.command {
                Command::InsertBefore { .. } => "Inserting new lines before a marker",
                Command::InsertAfter { .. } => "Inserting new lines after a marker",
                Command::Delete { .. } => "Deleting lines",
                Command::CreateFile { .. } => "Creating a new file",
                Command::RenameFile { .. } => "Renaming a file",
                Command::DeleteFile => "Deleting a file",
            }
        );
        println!("=> Reason: {}", change.reason);

        match &change.command {
            Command::CreateFile { new_lines } => {
                let file_path = Path::new(&change.filename);
                if file_path.exists() {
                    bail!("File already exists: {:?}", change.filename);
                }
                if let Some(parent) = file_path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("✗ Failed to create directory: {:?}", parent))?;
                }
                fs::write(file_path, new_lines.lines().join("\n")).with_context(|| {
                    format!("✗ Failed to create file: {}", change.filename.display())
                })?;
                println!(
                    "✓ Created file {} and inserted {} lines",
                    change.filename.display(),
                    new_lines.len()
                );
            }
            Command::RenameFile { new_filename } => {
                fs::rename(&change.filename, new_filename).with_context(|| {
                    format!("✗ Failed to rename file: {}", change.filename.display())
                })?;
                println!(
                    "✓ Renamed file: {} -> {}",
                    change.filename.display(),
                    new_filename.display()
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
                        "✓ Inserted {} lines into {}",
                        insert_lines.len(),
                        change.filename.display()
                    );
                } else {
                    bail!(
                        "Failed to find {} lines in {:?}",
                        marker_lines.len(),
                        change.filename.display()
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

                if marker_lines.len() == 0 && file_lines.is_empty() {
                    // This is the start of a new file
                    fs::write(&change.filename, insert_lines.lines().join("\n")).with_context(
                        || format!("✗ Failed to write to file: {:?}", change.filename),
                    )?;
                    println!(
                        "✓ Inserted {} lines into {}",
                        insert_lines.len(),
                        change.filename.display()
                    );
                } else if let Some(index) = find_in_file_lines(&file_lines, &marker_lines.lines()) {
                    let mut new_lines = file_lines[..=index + marker_lines.len() - 1].to_vec();
                    new_lines.extend(insert_lines.lines().iter().cloned());
                    new_lines.extend(file_lines[index + marker_lines.len()..].iter().cloned());
                    fs::write(&change.filename, new_lines.join("\n")).with_context(|| {
                        format!("✗ Failed to write to file: {:?}", change.filename)
                    })?;
                    println!(
                        "✓ Inserted {} lines into {}",
                        insert_lines.len(),
                        change.filename.display()
                    );
                } else {
                    bail!(
                        "Failed to find {} lines in {:?}",
                        marker_lines.len(),
                        change.filename.display()
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
                    println!(
                        "✓ Deleted {} lines in {:?}",
                        delete_lines.len(),
                        change.filename.display()
                    );
                } else {
                    bail!(
                        "Failed to find {} lines to delete in {:?}",
                        delete_lines.len(),
                        change.filename.display()
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
    let non_empty_needle: Vec<_> = needle
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if non_empty_needle.is_empty() {
        return Some(0);
    }

    let non_empty_file_lines: Vec<_> = file_lines
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if non_empty_needle.len() > non_empty_file_lines.len() {
        return None;
    }

    for (i, window) in non_empty_file_lines
        .windows(non_empty_needle.len())
        .enumerate()
    {
        if window == non_empty_needle {
            return Some(
                file_lines
                    .iter()
                    .take(i)
                    .filter(|s| !s.trim().is_empty())
                    .count(),
            );
        }
    }

    None
}
