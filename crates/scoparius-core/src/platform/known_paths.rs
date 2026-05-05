//! Known paths and protected directories across platforms.

use std::path::PathBuf;

/// Paths that should NEVER be included in cleanup operations.
pub fn system_protected_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            paths.push(PathBuf::from(&windir).join("System32"));
            paths.push(PathBuf::from(&windir).join("SysWOW64"));
            paths.push(PathBuf::from(&windir).join("WinSxS"));
        }
        paths.push(PathBuf::from(r"C:\Program Files"));
        paths.push(PathBuf::from(r"C:\Program Files (x86)"));
        paths.push(PathBuf::from(r"C:\ProgramData\Microsoft"));
    }

    #[cfg(target_os = "linux")]
    {
        paths.extend([
            PathBuf::from("/bin"),
            PathBuf::from("/sbin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/usr/sbin"),
            PathBuf::from("/usr/lib"),
            PathBuf::from("/etc"),
            PathBuf::from("/boot"),
            PathBuf::from("/proc"),
            PathBuf::from("/sys"),
            PathBuf::from("/dev"),
        ]);
    }

    #[cfg(target_os = "macos")]
    {
        paths.extend([
            PathBuf::from("/System"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/usr/sbin"),
            PathBuf::from("/usr/lib"),
            PathBuf::from("/private/var/db"),
        ]);
    }

    paths
}

/// Well-known cloud sync directories.
pub fn cloud_sync_dirs() -> Vec<(PathBuf, &'static str)> {
    let mut dirs = Vec::new();

    if let Some(home) = dirs::home_dir() {
        dirs.push((home.join("OneDrive"), "OneDrive"));
        dirs.push((home.join("Dropbox"), "Dropbox"));
        dirs.push((home.join("Google Drive"), "Google Drive"));
        dirs.push((home.join("iCloud Drive"), "iCloud"));
        dirs.push((home.join("MEGA"), "MEGA"));
        dirs.push((home.join("pCloud Drive"), "pCloud"));
    }

    dirs
}

/// Common temporary file directories.
pub fn common_temp_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(tmp) = std::env::var("TEMP") {
        dirs.push(PathBuf::from(tmp));
    }
    if let Ok(tmp) = std::env::var("TMP") {
        dirs.push(PathBuf::from(tmp));
    }

    dirs.push(std::env::temp_dir());

    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/tmp"));
        dirs.push(PathBuf::from("/var/tmp"));
    }

    dirs.sort();
    dirs.dedup();
    dirs
}

/// Common cache directories.
pub fn common_cache_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(cache) = dirs::cache_dir() {
        dirs.push(cache);
    }

    if let Some(home) = dirs::home_dir() {
        // Developer tool caches
        dirs.push(home.join(".cargo").join("registry"));
        dirs.push(home.join(".npm"));
        dirs.push(home.join(".cache"));
        dirs.push(home.join(".gradle").join("caches"));
        dirs.push(home.join(".m2").join("repository"));
        dirs.push(home.join(".nuget").join("packages"));

        #[cfg(target_os = "windows")]
        {
            if let Ok(local) = std::env::var("LOCALAPPDATA") {
                dirs.push(PathBuf::from(&local).join("pip").join("cache"));
                dirs.push(PathBuf::from(&local).join("yarn").join("Cache"));
            }
        }
    }

    dirs
}
