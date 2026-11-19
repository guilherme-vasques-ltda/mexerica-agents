//! Pastas descartáveis das fixtures, fora dos dados internos do Hub.

// Cada binário de integração usa só parte de `common`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub struct Workspaces {
    root: PathBuf,
}

impl Workspaces {
    pub fn new(home: &Path) -> Self {
        let root = home.with_extension("workspaces");
        std::fs::create_dir_all(&root).expect("criar as pastas de trabalho das fixtures");
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Workspaces {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
