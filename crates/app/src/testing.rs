//! What the app's tests share: folders of their own under the temp dir, which go once their test
//! has ended.

use std::cell::RefCell;
use std::path::PathBuf;

/// The folders the test running on this thread made ([`scratch_dir`]), removed when the thread
/// ends, whether the test passed or not.
struct Made(RefCell<Vec<PathBuf>>);

impl Drop for Made {
    fn drop(&mut self) {
        for dir in self.0.get_mut().drain(..) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

thread_local! {
    static MADE: Made = const { Made(RefCell::new(Vec::new())) };
}

/// A new, empty folder `sawit-<name>-<process>` under the temp dir, for the test running on this
/// thread, removed once that test has ended (seams m7).
///
/// Not when the caller's scope ends: a GPUI test's views outlive its body, which
/// `#[gpui_kit::test]` follows by dropping them, and a view that closes then still writes into its
/// folder (a canvas saves what it hadn't saved yet). A folder removed at the end of the body was
/// made again by that save, and nothing removed it then: a run of the app's tests left more than
/// a hundred in the temp dir. libtest runs each test on a thread of its own, which ends after all
/// that: the folder goes with it.
pub(crate) fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sawit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A thread that is ending (a destructor making a folder) can't take it: it stays.
    let _ = MADE.try_with(|made| made.0.borrow_mut().push(dir.clone()));
    dir
}

#[cfg(test)]
mod tests {
    use super::scratch_dir;

    /// A scratch folder stays for as long as its test runs, though the code that made it is done
    /// with it, and goes once the test's thread ends, with what was written into it last.
    #[test]
    fn a_scratch_folder_goes_once_its_test_has_ended() {
        let dir = std::thread::spawn(|| {
            let dir = scratch_dir("scratch-ends");
            assert!(dir.is_dir() && std::fs::read_dir(&dir).unwrap().next().is_none());
            // Written after the body that made it is done, as a view closing in the harness's
            // ending writes.
            std::fs::write(dir.join("late.txt"), "late").unwrap();
            dir
        })
        .join()
        .expect("the test's thread panicked");
        assert!(!dir.exists(), "{} was left behind", dir.display());
    }
}
