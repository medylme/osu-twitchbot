use std::io::{Write, stdin, stdout};

use indicatif::{ProgressBar, ProgressStyle};

use super::core::UpdateError;

const RELEASES_URL: &str = "https://github.com/medylme/osu-twitchbot/releases/tag";

#[cfg(all(target_os = "windows", not(debug_assertions)))]
fn alloc_console() {
    use windows::Win32::System::Console::AllocConsole;
    unsafe {
        let _ = AllocConsole();
    }
}

#[cfg(all(target_os = "windows", not(debug_assertions)))]
fn free_console() {
    use windows::Win32::System::Console::FreeConsole;
    unsafe {
        let _ = FreeConsole();
    }
}

fn prompt_open_release(version: &semver::Version, tag: &str, reason: &str) -> Result<(), UpdateError> {
    println!(
        "\n\x1b[33m!\x1b[0m New version v{} found, but {}.",
        version, reason
    );
    print!("Open release page in browser? [Y/n] ");
    let _ = stdout().flush();

    let mut input = String::new();
    if stdin().read_line(&mut input).is_ok() {
        let input = input.trim().to_lowercase();
        if input.is_empty() || input == "y" || input == "yes" {
            let url = format!("{}/{}", RELEASES_URL, tag);
            if open::that(&url).is_err() {
                println!("Failed to open browser. Visit: {}", url);
            }
        }
    }

    Err(UpdateError::UserDeclined)
}

pub fn run_startup_update_check() -> Result<(), UpdateError> {
    let rt = tokio::runtime::Runtime::new().map_err(UpdateError::Io)?;

    rt.block_on(async {
        #[cfg(all(target_os = "windows", not(debug_assertions)))]
        alloc_console();

        let client = reqwest::Client::new();

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.#969eff} {msg}")
                .unwrap(),
        );
        spinner.set_message("Checking for updates...");
        spinner.enable_steady_tick(std::time::Duration::from_millis(100));

        let release = match super::core::check_for_updates(&client).await {
            Ok(Some(release)) => {
                spinner.finish_and_clear();
                release
            }
            Ok(None) => {
                spinner.finish_and_clear();
                #[cfg(all(target_os = "windows", not(debug_assertions)))]
                free_console();
                return Ok(());
            }
            Err(_) => {
                spinner.finish_and_clear();
                #[cfg(all(target_os = "windows", not(debug_assertions)))]
                free_console();
                return Ok(());
            }
        };

        let result = perform_update(&client, &release).await;

        #[cfg(all(target_os = "windows", not(debug_assertions)))]
        free_console();

        result
    })
}

async fn perform_update(
    client: &reqwest::Client,
    release: &super::core::ReleaseInfo,
) -> Result<(), UpdateError> {
    // Check if checksum is available
    let (checksum_url, checksum_name) = match (&release.checksum_url, &release.checksum_name) {
        (Some(url), Some(name)) => (url.clone(), name.clone()),
        _ => {
            return prompt_open_release(
                &release.version,
                &release.tag_name,
                "could not verify signature (no checksum file)",
            );
        }
    };

    let temp_dir = tempfile::tempdir()?;
    let binary_path = temp_dir.path().join(&release.binary_name);
    let checksum_path = temp_dir.path().join(&checksum_name);

    // Download checksum file
    if let Err(_) = super::download::download_file(client, &checksum_url, &checksum_path, 0, |_| {}).await {
        return prompt_open_release(
            &release.version,
            &release.tag_name,
            "could not verify signature (failed to download checksum)",
        );
    }

    let checksum_content = match tokio::fs::read_to_string(&checksum_path).await {
        Ok(content) => content,
        Err(_) => {
            return prompt_open_release(
                &release.version,
                &release.tag_name,
                "could not verify signature (failed to read checksum)",
            );
        }
    };

    let expected_hash = match super::download::parse_checksum_file(&checksum_content, &release.binary_name) {
        Some(hash) => hash,
        None => {
            return prompt_open_release(
                &release.version,
                &release.tag_name,
                "could not verify signature (invalid checksum format)",
            );
        }
    };

    let pb = ProgressBar::new(release.size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg}\n{spinner:.#969eff} [{elapsed_precise}] [{wide_bar:.#969eff/white}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_message(format!(
        "New version available! Downloading v{}...",
        release.version
    ));

    super::download::download_file(
        client,
        &release.binary_url,
        &binary_path,
        release.size,
        |progress| {
            pb.set_position((progress * release.size as f32) as u64);
        },
    )
    .await?;

    pb.finish_and_clear();
    println!("\x1b[32m✓\x1b[0m Download complete");

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.#969eff} {msg}")
            .unwrap(),
    );
    spinner.set_message("Verifying...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    match super::download::verify_checksum(&binary_path, &expected_hash).await {
        Ok(true) => {
            spinner.finish_and_clear();
            println!("\x1b[32m✓\x1b[0m Verified");
        }
        Ok(false) | Err(_) => {
            spinner.finish_and_clear();
            println!("\x1b[31m✗\x1b[0m Verification failed");
            return prompt_open_release(
                &release.version,
                &release.tag_name,
                "could not verify signature (checksum mismatch)",
            );
        }
    }

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.#969eff} {msg}")
            .unwrap(),
    );
    spinner.set_message("Installing...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    super::install::install_update(&binary_path)?;

    spinner.finish_and_clear();
    println!("\x1b[32m✓\x1b[0m Installed");

    println!("\nRestarting...");
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    super::install::restart_application()?;

    Ok(())
}
