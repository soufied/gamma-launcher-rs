use crate::error::Result;
use crate::report::Reporter;
use md5::{Digest, Md5};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const READ_BUFFER_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct HashOutcome {
    pub expected: String,
    pub computed: String,
}

impl HashOutcome {
    pub fn matches(&self) -> bool {
        self.computed.eq_ignore_ascii_case(&self.expected)
    }
}

pub fn check_hash(
    file: &Path,
    checksum: &str,
    reporter: &Reporter,
    label: &str,
) -> Result<HashOutcome> {
    let mut handle = File::open(file)?;
    let total = handle.metadata()?.len();
    let mut hasher = Md5::new();
    let mut buffer = vec![0u8; READ_BUFFER_SIZE];
    let mut read_total: u64 = 0;
    let id = file.to_string_lossy().to_string();

    reporter.task_started(&id, label);

    loop {
        if let Err(error) = reporter.checkpoint() {
            reporter.task_finished(&id, Some(error.to_string()));
            return Err(error);
        }

        let read = handle.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        read_total += read as u64;
        reporter.progress(&id, label, read_total, Some(total));
    }

    let outcome = HashOutcome {
        expected: checksum.to_string(),
        computed: hex::encode(hasher.finalize()),
    };

    reporter.task_finished(
        &id,
        if outcome.matches() {
            None
        } else {
            Some(format!(
                "hash mismatch for {}: expected {}, got {}",
                file.display(),
                outcome.expected,
                outcome.computed
            ))
        },
    );

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_ignores_digest_casing() {
        let outcome = HashOutcome {
            expected: "D41D8CD98F00B204E9800998ECF8427E".to_string(),
            computed: "d41d8cd98f00b204e9800998ecf8427e".to_string(),
        };

        assert!(outcome.matches());
    }

    #[test]
    fn different_digests_do_not_match() {
        let outcome = HashOutcome {
            expected: "aaaa".to_string(),
            computed: "bbbb".to_string(),
        };

        assert!(!outcome.matches());
    }
}
