//! Percurso binário limitado e comparação exaustiva, sem autoridade de execução.
use super::super::{
    Blob, Entrada, MAX_ARQUIVO_BYTES, MAX_ARQUIVOS, MAX_TOTAL_BYTES, Manifesto, Pacote, digest_manifesto, validar_path,
};
use super::{SelecaoGit, git::Process, odb::Private};
use anyhow::{Context as _, Result, ensure};
use sha2::{Digest as _, Sha256};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static BUSY: AtomicBool = AtomicBool::new(false);

pub(super) struct Budget<'a> {
    deadline: Instant,
    cancel: &'a AtomicBool,
    out: Cell<usize>,
    err: Cell<usize>,
    launches: Cell<usize>,
    #[cfg(test)]
    pub(super) copy_observer: Cell<Option<fn(i32)>>,
}
impl<'a> Budget<'a> {
    pub fn new(cancel: &'a AtomicBool) -> Self {
        Self {
            deadline: Instant::now() + Duration::from_secs(30),
            cancel,
            out: Cell::new(0),
            err: Cell::new(0),
            launches: Cell::new(0),
            #[cfg(test)]
            copy_observer: Cell::new(None),
        }
    }
    #[cfg(test)]
    pub(super) fn deadline_for_test(&mut self, duration: Duration) {
        self.deadline = Instant::now() + duration;
    }
    pub fn check(&self) -> Result<()> {
        ensure!(!self.cancel.load(Ordering::Acquire), "snapshot_git_cancelled");
        ensure!(Instant::now() < self.deadline, "snapshot_git_timeout");
        Ok(())
    }
    pub fn launch(&self) -> Result<()> {
        self.check()?;
        Self::charge(&self.launches, 1, 4, "snapshot_git_process_limit")
    }
    pub fn stdout(&self, n: usize) -> Result<()> {
        Self::charge(&self.out, n, 80 * 1024 * 1024, "snapshot_git_stdout_limit")
    }
    pub fn stderr(&self, n: usize) -> Result<()> {
        Self::charge(&self.err, n, 64 * 1024, "snapshot_git_stderr_limit")
    }
    fn charge(counter: &Cell<usize>, n: usize, maximum: usize, code: &'static str) -> Result<()> {
        let next = counter.get().checked_add(n).context("snapshot_overflow")?;
        ensure!(next <= maximum, "{code}");
        counter.set(next);
        Ok(())
    }
}

struct Slot;
impl Slot {
    fn acquire() -> Result<Self> {
        ensure!(BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok(), "snapshot_git_busy");
        Ok(Self)
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

struct Object {
    kind: &'static str,
    oid: String,
    file: String,
}

struct Reader<'a, 'b> {
    process: Process<'a, 'b>,
    private: &'a Private,
    objects: Vec<Object>,
    metadata: usize,
    blob_bytes: u64,
    spool_bytes: usize,
}
impl Reader<'_, '_> {
    fn object(&mut self, oid: &str, kind: &'static str) -> Result<Vec<u8>> {
        ensure!(self.objects.len() < 12290, "snapshot_git_object_limit");
        self.process.request(oid)?;
        let line = self.process.line()?;
        let fields: Vec<_> = line.split(' ').collect();
        ensure!(fields.len() == 3 && fields[0] == oid && fields[1] == kind, "snapshot_git_object_header");
        let decimal = fields[2];
        ensure!(
            !decimal.is_empty()
                && decimal.bytes().all(|b| b.is_ascii_digit())
                && (decimal == "0" || !decimal.starts_with('0')),
            "snapshot_git_size"
        );
        let length: usize = decimal.parse()?;
        match kind {
            "commit" | "tree" => {
                ensure!(
                    length <= if kind == "commit" { 64 * 1024 } else { 2 * 1024 * 1024 },
                    "snapshot_git_metadata_limit"
                );
                self.metadata = self.metadata.checked_add(length).context("snapshot_overflow")?;
                ensure!(self.metadata <= 8 * 1024 * 1024, "snapshot_git_metadata_limit");
            }
            "blob" => {
                let size = u64::try_from(length)?;
                ensure!(size <= MAX_ARQUIVO_BYTES, "snapshot_file_bytes");
                self.blob_bytes = self.blob_bytes.checked_add(size).context("snapshot_overflow")?;
                ensure!(self.blob_bytes <= MAX_TOTAL_BYTES, "snapshot_total_bytes");
            }
            _ => anyhow::bail!("snapshot_git_type"),
        }
        self.spool_bytes = self.spool_bytes.checked_add(length).context("snapshot_overflow")?;
        ensure!(self.spool_bytes <= 80 * 1024 * 1024, "snapshot_git_spool_limit");
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length)?;
        bytes.resize(length, 0);
        self.process.exact(&mut bytes)?;
        let mut newline = [0];
        self.process.exact(&mut newline)?;
        ensure!(newline == *b"\n", "snapshot_git_object_framing");
        let file = self.private.body(self.objects.len(), &bytes)?;
        self.objects.push(Object { kind, oid: oid.to_owned(), file });
        Ok(bytes)
    }
}

pub(super) fn conferir<'a>(
    pacote: &'a Pacote,
    selection: &SelecaoGit<'_>,
    origin: &Path,
    cancel: &AtomicBool,
) -> Result<super::super::PacoteConferidoManifesto<'a>> {
    let _slot = Slot::acquire()?;
    let budget = Budget::new(cancel);
    budget.check()?;
    let manifesto = super::super::conferir_manifesto(pacote)?;
    budget.check()?;
    let mut private = Private::create()?;
    private.copy(origin, selection, &budget)?;
    let generated = read(&private, selection, &budget)?;
    ensure!(pacote.manifesto.repo == selection.repo, "snapshot_git_repo");
    ensure!(generated.manifesto.digest_total == pacote.manifesto.digest_total, "snapshot_git_manifest");
    ensure!(generated.blobs.len() == pacote.blobs.len(), "snapshot_git_entries");
    ensure!(generated.manifesto.arquivos.len() == pacote.manifesto.arquivos.len(), "snapshot_git_entries");
    for (actual, expected) in generated.manifesto.arquivos.iter().zip(&pacote.manifesto.arquivos) {
        ensure!(
            actual.path == expected.path
                && actual.mode == expected.mode
                && actual.size == expected.size
                && actual.digest == expected.digest,
            "snapshot_git_entry"
        );
    }
    for (actual, expected) in generated.blobs.iter().zip(&pacote.blobs) {
        ensure!(actual.path == expected.path && actual.bytes == expected.bytes, "snapshot_git_bytes");
    }
    budget.check()?;
    private.cleanup()?;
    budget.check()?;
    Ok(manifesto)
}

fn read(private: &Private, selection: &SelecaoGit<'_>, budget: &Budget<'_>) -> Result<Pacote> {
    let process = Process::start(private, &["cat-file", "--batch"], budget)?;
    let mut reader = Reader { process, private, objects: Vec::new(), metadata: 0, blob_bytes: 0, spool_bytes: 0 };
    let commit = reader.object(selection.commit, "commit")?;
    let tree = commit_tree(&commit, selection.formato.largura())?;
    ensure!(tree == selection.tree, "snapshot_git_commit_tree");
    let mut pending = vec![(String::new(), tree, 0_usize)];
    let mut directories = 1_usize;
    let mut files = BTreeMap::new();
    while let Some((prefix, oid, depth)) = pending.pop() {
        budget.check()?;
        let bytes = reader.object(&oid, "tree")?;
        ensure!(prefix.is_empty() || !bytes.is_empty(), "snapshot_git_empty_subtree");
        let mut offset = 0;
        let mut names = BTreeSet::new();
        while offset < bytes.len() {
            budget.check()?;
            let space = offset + bytes[offset..].iter().position(|b| *b == b' ').context("snapshot_git_tree")?;
            let mode = match &bytes[offset..space] {
                b"40000" => 0o040_000,
                b"100644" => 0o100_644,
                b"100755" => 0o100_755,
                _ => anyhow::bail!("snapshot_git_mode"),
            };
            let start = space + 1;
            let end = start + bytes[start..].iter().position(|b| *b == 0).context("snapshot_git_tree")?;
            ensure!(end > start && !bytes[start..end].contains(&b'/'), "snapshot_git_name");
            let path_length = prefix.len().checked_add(end - start).context("snapshot_overflow")?;
            ensure!(path_length <= super::super::MAX_PATH_BYTES, "snapshot_path");
            let name = std::str::from_utf8(&bytes[start..end])?;
            ensure!(names.insert(name.to_owned()), "snapshot_git_duplicate");
            let path = format!("{prefix}{name}");
            validar_path(&path)?;
            let oid_end = end.checked_add(1 + selection.formato.largura() / 2).context("snapshot_overflow")?;
            ensure!(oid_end <= bytes.len(), "snapshot_git_tree");
            let oid = hexadecimal(&bytes[end + 1..oid_end]);
            offset = oid_end;
            if mode == 0o040_000 {
                directories = directories.checked_add(1).context("snapshot_overflow")?;
                ensure!(directories <= 8192 && depth < 64, "snapshot_git_directory_limit");
                pending.push((format!("{path}/"), oid, depth + 1));
            } else {
                ensure!(files.len() < MAX_ARQUIVOS, "snapshot_file_limit");
                let body = reader.object(&oid, "blob")?;
                ensure!(files.insert(path, (mode, body)).is_none(), "snapshot_git_duplicate");
            }
        }
    }
    let Reader { process, objects, .. } = reader;
    process.finish(b"", 0)?; // EOF/status e nenhuma sobra antes de iniciar outro filho
    private.seal_spool()?;
    for kind in ["commit", "tree", "blob"] {
        let batch: Vec<_> = objects.iter().filter(|object| object.kind == kind).collect();
        if batch.is_empty() {
            continue;
        }
        let mut input = String::new();
        for object in &batch {
            writeln!(&mut input, "spool/{}", object.file)?;
        }
        let output_size = batch.len().checked_mul(selection.formato.largura() + 1).context("snapshot_overflow")?;
        let process = Process::start(private, &["hash-object", "--stdin-paths", "--no-filters", "-t", kind], budget)?;
        let output = process.finish(input.as_bytes(), output_size)?;
        ensure!(output.len() == output_size, "snapshot_git_rehash_count");
        for (line, object) in output.chunks_exact(selection.formato.largura() + 1).zip(batch) {
            ensure!(
                line.last() == Some(&b'\n') && &line[..line.len() - 1] == object.oid.as_bytes(),
                "snapshot_git_rehash"
            );
        }
    }
    let mut package = Pacote {
        manifesto: Manifesto {
            version: 1,
            repo: selection.repo.to_owned(),
            commit: selection.commit.to_owned(),
            tree: selection.tree.to_owned(),
            arquivos: Vec::new(),
            digest_total: [0; 32],
        },
        blobs: Vec::new(),
    };
    for (path, (mode, bytes)) in files {
        package.manifesto.arquivos.push(Entrada {
            path: path.clone(),
            mode,
            digest: Sha256::digest(&bytes).into(),
            size: u64::try_from(bytes.len())?,
        });
        package.blobs.push(Blob { path, bytes });
    }
    package.manifesto.digest_total = digest_manifesto(&package.manifesto)?;
    super::super::conferir_manifesto(&package)?;
    budget.check()?;
    Ok(package)
}

fn commit_tree(bytes: &[u8], width: usize) -> Result<String> {
    let end = bytes.windows(2).position(|w| w == b"\n\n").context("snapshot_git_commit")?;
    let mut trees = bytes[..end].split(|b| *b == b'\n').filter_map(|line| line.strip_prefix(b"tree "));
    let tree = std::str::from_utf8(trees.next().context("snapshot_git_commit_tree")?)?;
    ensure!(
        bytes.starts_with(b"tree ") && super::super::oid(tree) && tree.len() == width && trees.next().is_none(),
        "snapshot_git_commit_tree"
    );
    Ok(tree.to_owned())
}

fn hexadecimal(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    result
}
