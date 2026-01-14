use indicatif::{ProgressBar, ProgressStyle};

use super::core::UpdateError;

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

        perform_update(&client, &release).await
    })
}

async fn perform_update(
    client: &reqwest::Client,
    release: &super::core::ReleaseInfo,
) -> Result<(), UpdateError> {
    let temp_dir = tempfile::tempdir()?;
    let binary_path = temp_dir.path().join(&release.binary_name);
    let checksum_path = temp_dir.path().join(&release.checksum_name);

    super::download::download_file(client, &release.checksum_url, &checksum_path, 0, |_| {})
        .await?;

    let checksum_content = tokio::fs::read_to_string(&checksum_path).await?;
    let expected_hash =
        super::download::parse_checksum_file(&checksum_content, &release.binary_name)
            .ok_or(UpdateError::ChecksumNotFound)?;

    let pb = ProgressBar::new(release.size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg}\n{spinner:.#969eff} [{elapsed_precise}] [{wide_bar:.#969eff/white}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_message(format!("Downloading v{}...", release.version));

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

    if !super::download::verify_checksum(&binary_path, &expected_hash).await? {
        spinner.finish_and_clear();
        println!("\x1b[31m✗\x1b[0m Verification failed");
        return Err(UpdateError::ChecksumMismatch);
    }

    spinner.finish_and_clear();
    println!("\x1b[32m✓\x1b[0m Verified");

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
