//! Where the editor was, so it comes back there: the open file, the caret and
//! the scroll. The file is saved whenever it changes (opened, saved as, a new
//! document), the caret and scroll when the window exits (`on_exit`, which
//! `ccectl restart-app` runs as any close does); a launch reopens it — after a
//! restart, at login, or when reopened.
//!
//! **A restart replays the same command line**, so a window opened as
//! `cce-text-editor a.txt` would always come back on a.txt, whatever was opened
//! since. The file therefore also records the file the command line named
//! (`launched`): the same command line again, or one naming nothing, is this
//! window reopening, and the saved file wins; a command line naming a different
//! file is a new request, and it wins. A saved file that has gone is not
//! reopened, and the caret and scroll only go back into the file they came
//! from. Restoring only reads: nothing here creates or writes a document.
//!
//! `cce_state_dir()/text-editor/state.json`, written whole to a temporary file
//! and renamed over, so a crash mid-write leaves the last one.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Saved {
    /// The open file, absolute (`None`: an untitled document).
    file: Option<PathBuf>,
    /// The file the command line named when this was saved, absolute.
    launched: Option<PathBuf>,
    /// The caret, in chars.
    #[serde(default)]
    cursor: usize,
    #[serde(default)]
    scroll_y: f32,
}

/// What a launch opens: the file, and the caret and scroll in it.
#[derive(Debug, Default, PartialEq)]
pub struct Place {
    pub file: Option<PathBuf>,
    pub cursor: usize,
    pub scroll_y: f32,
}

/// The file this run's command line named.
static LAUNCHED: OnceLock<Option<PathBuf>> = OnceLock::new();

fn path() -> PathBuf {
    cce_ui::config::cce_state_dir().join("text-editor").join("state.json")
}

fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

/// What to open, given the file the command line named: the saved place when
/// this command line is the one it was saved under (or names nothing), else
/// the command line's file.
pub fn initial(argv_file: Option<&Path>) -> Place {
    let launched = argv_file.map(absolute);
    let _ = LAUNCHED.set(launched.clone());
    let saved = std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str::<Saved>(&s).ok());
    choose(launched.as_deref(), saved.as_ref(), Path::exists)
}

fn choose(launched: Option<&Path>, saved: Option<&Saved>, exists: impl Fn(&Path) -> bool) -> Place {
    let place = |s: &Saved, file: Option<PathBuf>| Place { file, cursor: s.cursor, scroll_y: s.scroll_y };
    let reopening = saved.filter(|s| launched.is_none() || s.launched.as_deref() == launched);
    if let Some(s) = reopening.filter(|s| s.file.as_deref().is_none_or(&exists)) {
        return place(s, s.file.clone());
    }
    // As launched; the saved caret only if it was saved in this very file.
    let file = launched.map(Path::to_path_buf);
    match saved.filter(|s| file.is_some() && s.file == file) {
        Some(s) => place(s, file),
        None => Place { file, ..Place::default() },
    }
}

/// The open file changed, or the window is closing with the caret and scroll
/// where they are.
pub fn save(file: Option<&Path>, cursor: usize, scroll_y: f32) {
    let saved = Saved { file: file.map(absolute), launched: LAUNCHED.get().cloned().flatten(), cursor, scroll_y };
    let path = path();
    let Some(dir) = path.parent() else { return };
    let tmp = path.with_extension("json.tmp");
    let written = std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&tmp, serde_json::to_vec(&saved).unwrap_or_default()))
        .and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        eprintln!("[cce-text-editor] cannot save the open file to {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(file: Option<&str>, launched: Option<&str>) -> Saved {
        Saved { file: file.map(PathBuf::from), launched: launched.map(PathBuf::from), cursor: 42, scroll_y: 300.0 }
    }

    fn at(file: Option<&str>, cursor: usize, scroll_y: f32) -> Place {
        Place { file: file.map(PathBuf::from), cursor, scroll_y }
    }

    const ALL: fn(&Path) -> bool = |_| true;

    #[test]
    fn the_same_command_line_reopens_where_it_was() {
        // Launched on a.txt, opened b.txt, restarted with the same argv: b.txt.
        let s = saved(Some("/b.txt"), Some("/a.txt"));
        assert_eq!(choose(Some(Path::new("/a.txt")), Some(&s), ALL), at(Some("/b.txt"), 42, 300.0));
        // A plain launch reopens whatever was open, however that was launched.
        assert_eq!(choose(None, Some(&s), ALL), at(Some("/b.txt"), 42, 300.0));
        // An untitled document stays untitled.
        assert_eq!(choose(None, Some(&saved(None, None)), ALL), at(None, 42, 300.0));
    }

    #[test]
    fn a_command_line_naming_another_file_wins() {
        let s = saved(Some("/b.txt"), Some("/a.txt"));
        assert_eq!(choose(Some(Path::new("/c.txt")), Some(&s), ALL), at(Some("/c.txt"), 0, 0.0));
        // ...but naming the file that was open keeps its place.
        let s = saved(Some("/c.txt"), None);
        assert_eq!(choose(Some(Path::new("/c.txt")), Some(&s), ALL), at(Some("/c.txt"), 42, 300.0));
    }

    #[test]
    fn nothing_saved_opens_as_launched() {
        assert_eq!(choose(Some(Path::new("/c.txt")), None, ALL), at(Some("/c.txt"), 0, 0.0));
        assert_eq!(choose(None, None, ALL), Place::default());
    }

    #[test]
    fn a_saved_file_that_has_gone_is_not_reopened() {
        let gone = |p: &Path| p != Path::new("/b.txt");
        let s = saved(Some("/b.txt"), Some("/a.txt"));
        assert_eq!(choose(Some(Path::new("/a.txt")), Some(&s), gone), at(Some("/a.txt"), 0, 0.0));
        assert_eq!(choose(None, Some(&s), gone), Place::default());
    }
}
