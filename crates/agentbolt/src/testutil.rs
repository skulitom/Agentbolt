use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub struct TempDir(PathBuf);
impl TempDir {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "agentbolt-adapter-test-{}-{now}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let parent = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let path = std::fs::canonicalize(&self.0).unwrap();
        assert!(
            path.starts_with(parent)
                && path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("agentbolt-adapter-test-")
        );
        std::fs::remove_dir_all(&path).unwrap();
    }
}
