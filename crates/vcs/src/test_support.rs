//! Shared git repository fixture for the crate's tests.

use std::fs;
use std::path::Path;

use git2::Repository;

pub(crate) struct TestRepo {
    _dir: tempfile::TempDir,
    pub(crate) repo: Repository,
}

impl TestRepo {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "test").unwrap();
            config.set_str("user.email", "test@example.com").unwrap();
        }
        Self { _dir: dir, repo }
    }

    pub(crate) fn root(&self) -> &Path {
        self.repo.workdir().unwrap()
    }

    pub(crate) fn write(&self, rel: &str, content: &str) {
        let path = self.root().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    pub(crate) fn remove(&self, rel: &str) {
        fs::remove_file(self.root().join(rel)).unwrap();
    }

    pub(crate) fn stage_all(&self) {
        let mut index = self.repo.index().unwrap();
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None).unwrap();
        // `add_all` does not drop deleted files from the index.
        index.update_all(["*"].iter(), None).unwrap();
        index.write().unwrap();
    }

    pub(crate) fn commit(&self, message: &str) -> git2::Oid {
        let sig = self.repo.signature().unwrap();
        self.commit_with(&sig, message)
    }

    pub(crate) fn commit_as(&self, name: &str, email: &str, message: &str) -> git2::Oid {
        let sig = git2::Signature::now(name, email).unwrap();
        self.commit_with(&sig, message)
    }

    fn commit_with(&self, sig: &git2::Signature<'_>, message: &str) -> git2::Oid {
        self.stage_all();
        let mut index = self.repo.index().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = self.repo.find_tree(tree_id).unwrap();
        let parent = self.repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<_> = parent.iter().collect();
        self.repo.commit(Some("HEAD"), sig, sig, message, &tree, &parents).unwrap()
    }

    pub(crate) fn branch(&self, name: &str) {
        let head = self.repo.head().unwrap().peel_to_commit().unwrap();
        self.repo.branch(name, &head, false).unwrap();
    }

    pub(crate) fn checkout(&self, name: &str) {
        let (object, reference) = self.repo.revparse_ext(name).unwrap();
        self.repo.checkout_tree(&object, None).unwrap();
        self.repo.set_head(reference.unwrap().name().unwrap()).unwrap();
    }
}
