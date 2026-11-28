use aho_corasick::{AhoCorasick, MatchKind};
use anyhow::{Result, ensure};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use zeroize::Zeroizing;

pub(super) struct Mask {
    search: AhoCorasick,
}

impl Mask {
    pub(super) fn from_store(store: &crate::store::Store) -> Result<Option<Self>> {
        let values: Vec<_> = crate::market::logins::all(store)?
            .into_iter()
            .filter(|login| !login.password.is_empty())
            .map(|login| Zeroizing::new(login.password))
            .collect();
        if values.is_empty() {
            return Ok(None);
        }
        let search = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostLongest)
            .build(values.iter().map(|value| value.as_bytes()))?;
        Ok(Some(Self { search }))
    }

    pub(super) fn name(&self, name: String) -> Result<String> {
        if !self.search.is_match(name.as_bytes()) {
            return Ok(name);
        }
        ensure!(!self.search.is_match(b"arquivo"), "file could not be shared safely");
        Ok("arquivo".into())
    }

    pub(super) fn copy(
        &self,
        input: &mut impl Read,
        output: &mut impl Write,
        cancelled: &AtomicBool,
    ) -> Result<(u64, u64, String)> {
        let mut raw = Zeroizing::new(Vec::new());
        let mut buffer = Zeroizing::new(vec![0; super::CHUNK_SIZE]);
        loop {
            ensure!(!cancelled.load(Ordering::Acquire), "file sharing cancelled");
            let count = input.read(&mut buffer[..])?;
            if count == 0 {
                break;
            }
            ensure!(raw.len() as u64 + count as u64 <= super::MAX_FILE, "file exceeds 100 MiB");
            raw.extend_from_slice(&buffer[..count]);
        }
        let raw_size = raw.len() as u64;
        if !self.search.is_match(&raw[..]) {
            let (size, digest) = super::copy(&mut &raw[..], output, cancelled)?;
            return Ok((raw_size, size, digest));
        }
        let text = std::str::from_utf8(&raw)
            .ok()
            .filter(|text| text.chars().all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t')));
        ensure!(text.is_some(), "file could not be shared safely");
        let mut clean = Zeroizing::new(Vec::new());
        let mut end = 0;
        for found in self.search.find_iter(&raw[..]) {
            ensure!(!cancelled.load(Ordering::Acquire), "file sharing cancelled");
            let marker = b"[cofre:ITEM]";
            let preceding = &raw[end..found.start()];
            ensure!((clean.len() + preceding.len() + marker.len()) as u64 <= super::MAX_FILE, "file exceeds 100 MiB");
            clean.extend_from_slice(preceding);
            clean.extend_from_slice(marker);
            end = found.end();
        }
        ensure!((clean.len() + raw.len() - end) as u64 <= super::MAX_FILE, "file exceeds 100 MiB");
        clean.extend_from_slice(&raw[end..]);
        ensure!(!self.search.is_match(&clean[..]), "file could not be shared safely");
        let (size, digest) = super::copy(&mut &clean[..], output, cancelled)?;
        Ok((raw_size, size, digest))
    }
}
