use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use futures::executor::block_on;
use futures::future::join_all;
use rayon::prelude::*;
use rayon::iter::ParallelIterator;
use std::io::BufWriter;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Arc;

const MAX_FILE_SIZE: u64 = 1_000_000_000; // 1gb filr size
const FILES_PER_THREAD: usize = 500;

fn extract_account_info(input_file: PathBuf, pb: Arc<ProgressBar>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Ok(file) = File::open(&input_file) {
        let reader = BufReader::new(file);

        let url_regex = regex::Regex::new(r"(http|https|android|ftp)://([^\s:]+)").unwrap();
        let credential_regex = regex::Regex::new(r"([^:\s]+)$").unwrap();

        let mut url = String::new();
        let mut login = String::new();
        let mut password = String::new();

        let mut skip_account = false;

        for line in reader.lines().filter_map(|l| l.ok()) {
            if let Some(url_match) = url_regex.captures(&line) {
                url = url_match[0].to_string();
                login.clear();
                password.clear();
                skip_account = false;
            } else if let Some(credential_match) = credential_regex.captures(&line) {
                if login.is_empty() {
                    login = credential_match[1].to_string();
                } else if password.is_empty() {
                    password = credential_match[1].to_string();
                }
            } else if line.trim().is_empty() && (login.is_empty() || password.is_empty()) {
                skip_account = true;
            }

            if !url.is_empty() && !login.is_empty() && !password.is_empty() && !skip_account {
                let formatted_line = format!("{}:{}:{}", url, login, password);
                lines.push(formatted_line);

                url.clear();
                login.clear();
                password.clear();
            }
        }

        pb.inc(1);
    } else {
        println!("Failed to open file: {:?}", input_file);
    }

    lines
}

async fn process_batch(input_files: Vec<PathBuf>, pb: Arc<ProgressBar>) -> Vec<String> {
    let lines: Vec<Vec<String>> = input_files
        .par_iter()
        .map(|input_file| extract_account_info(input_file.clone(), pb.clone()))
        .collect();

    lines.into_iter().flatten().collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        println!("Usage: ./name.exe <input_folder> <output_folder>");
        return;
    }

    let input_folder = PathBuf::from(&args[1]);
    let output_folder = PathBuf::from(&args[2]);

    if let Err(err) = fs::create_dir_all(&output_folder) {
        println!("Failed to create output folder: {:?}", err);
        return;
    }

    let input_files: Vec<PathBuf> = fs::read_dir(&input_folder)
        .unwrap_or_else(|_| {
            println!("Failed to read input folder: {:?}", input_folder);
            std::process::exit(1);
        })
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                && entry.path().extension().map_or(false, |ext| ext == "txt")
        })
        .map(|entry| entry.path())
        .collect();

    let batch_size = FILES_PER_THREAD;

    let pb = Arc::new(ProgressBar::new(input_files.len() as u64));
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%)")
            .progress_chars("#>-"),
    );

    let mut tasks = vec![];
    for batch_files in input_files.chunks(batch_size) {
        let pb = pb.clone();
        let task = process_batch(batch_files.to_vec(), pb);
        tasks.push(task);
    }

    let output_data: Vec<Vec<String>> = block_on(async {
        let results = join_all(tasks).await;
        pb.finish_with_message("Processing complete!");
        results
    });

    pb.set_length(output_data.len() as u64);

    let output_file_path = output_folder.join("output.txt");
    let mut file_index = 0;
    let mut current_file_size = 0;
    let mut output_writer = BufWriter::new(File::create(&output_file_path).unwrap());

    for batch_data in output_data {
        for line in batch_data {
            let line_size = line.len() as u64 + 1;

            if current_file_size + line_size > MAX_FILE_SIZE {
                output_writer.flush().unwrap();
                current_file_size = 0;
                file_index += 1;
                let output_file_path = output_folder.join(format!("output{}.txt", file_index));

                let output_file = match File::create(&output_file_path) {
                    Ok(file) => file,
                    Err(err) => {
                        println!("Failed to create output file: {:?}", err);
                        return;
                    }
                };

                output_writer = BufWriter::new(output_file);
            }

            writeln!(output_writer, "{}", line).unwrap();
            current_file_size += line_size;
        }
    }

    output_writer.flush().unwrap();

    println!("Account information extracted and saved successfully!");
}
