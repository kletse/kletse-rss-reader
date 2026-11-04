//! The app was called Kletse RSS. What it kept in `kletse-rss` folders moves to `onward` the first time it's looked for,
//! so an update keeps your servers and prefs.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// The app's folder under `base`. If the move fails, the old folder goes on being used.
pub fn dir(base: &Path) -> PathBuf {
    let (old, new) = (base.join("kletse-rss"), base.join("onward"));
    if new.exists() || !old.exists() {
        return new;
    }
    match fs::rename(&old, &new) {
        Ok(()) => new,
        Err(e) => {
            eprintln!("onward: could not move {} to {}: {e}", old.display(), new.display());
            old
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_folder_moves_once() {
        let base = tempfile::tempdir().expect("a temp dir");
        fs::create_dir(base.path().join("kletse-rss")).expect("the old folder");
        fs::write(base.path().join("kletse-rss").join("config.toml"), "x").expect("a file in it");
        let dir = dir(base.path());
        assert_eq!(dir, base.path().join("onward"));
        assert!(dir.join("config.toml").exists());
        assert!(!base.path().join("kletse-rss").exists());
    }

    #[test]
    fn a_new_folder_wins_and_nothing_is_made_up() {
        let base = tempfile::tempdir().expect("a temp dir");
        assert_eq!(dir(base.path()), base.path().join("onward"));
        assert!(!base.path().join("onward").exists(), "only a save makes the folder");
        fs::create_dir(base.path().join("kletse-rss")).expect("the old folder");
        fs::create_dir(base.path().join("onward")).expect("the new folder");
        assert_eq!(dir(base.path()), base.path().join("onward"));
        assert!(base.path().join("kletse-rss").exists(), "left alone");
    }
}
