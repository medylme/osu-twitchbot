use std::path::{Path, PathBuf};

use super::core::UpdateError;

pub fn get_current_exe() -> Result<PathBuf, UpdateError> {
    std::env::current_exe().map_err(UpdateError::Io)
}

pub fn cleanup_old_binary() {
    if let Ok(current_exe) = get_current_exe() {
        let old_path = current_exe.with_extension("old");
        try_remove_file(&old_path);

        #[cfg(windows)]
        {
            let exe_old = current_exe.with_extension("exe.old");
            try_remove_file(&exe_old);
        }
    }
}

fn try_remove_file(path: &Path) {
    if !path.exists() {
        return;
    }

    for _ in 0..5 {
        if std::fs::remove_file(path).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

pub fn install_update(new_binary: &Path) -> Result<(), UpdateError> {
    let current_exe = get_current_exe()?;

    #[cfg(target_os = "windows")]
    {
        install_windows(&current_exe, new_binary)?;
    }

    #[cfg(target_os = "linux")]
    {
        install_linux(&current_exe, new_binary)?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn install_windows(current_exe: &Path, new_binary: &Path) -> Result<(), UpdateError> {
    let backup_path = current_exe.with_extension("exe.old");

    if backup_path.exists() {
        std::fs::remove_file(&backup_path)?;
    }

    std::fs::rename(current_exe, &backup_path)?;
    std::fs::copy(new_binary, current_exe)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_linux(current_exe: &Path, new_binary: &Path) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt;

    let backup_path = current_exe.with_extension("old");

    if backup_path.exists() {
        std::fs::remove_file(&backup_path)?;
    }

    std::fs::rename(current_exe, &backup_path)?;
    std::fs::copy(new_binary, current_exe)?;

    let mut perms = std::fs::metadata(current_exe)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(current_exe, perms)?;

    Ok(())
}

pub fn restart_application() -> Result<(), UpdateError> {
    let current_exe = get_current_exe()?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        Command::new(&current_exe)
            .args(&args)
            .spawn()
            .map_err(|e| UpdateError::Restart(e.to_string()))?;
        std::process::exit(0);
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        use std::process::Command;

        let err = Command::new(&current_exe).args(&args).exec();
        Err(UpdateError::Restart(err.to_string()))
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err(UpdateError::Restart("Unsupported platform".to_string()))
    }
}
