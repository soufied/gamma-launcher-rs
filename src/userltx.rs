use crate::error::{LauncherError, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BindTable {
    pub kind: String,
    pub entries: Vec<(String, String)>,
}

impl BindTable {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            entries: Vec::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| k == key) {
            entry.1 = value;
        } else {
            self.entries.push((key.to_string(), value));
        }
    }

    pub fn to_azerty_layout(&mut self) {
        for (_, v) in self.entries.iter_mut() {
            if let Some(mapped) = azerty_map(v) {
                *v = mapped.to_string();
            }
        }
    }

    pub fn to_dvorak_layout(&mut self) {
        for (_, v) in self.entries.iter_mut() {
            if let Some(mapped) = dvorak_map(v) {
                *v = mapped.to_string();
            }
        }
    }
}

fn azerty_map(key: &str) -> Option<&'static str> {
    match key {
        "kW" => Some("kZ"),
        "kA" => Some("kQ"),
        "kQ" => Some("kA"),
        "kM" => Some("kCOMMA"),
        _ => None,
    }
}

fn dvorak_map(key: &str) -> Option<&'static str> {
    match key {
        "kW" => Some("kCOMMA"),
        "kS" => Some("kO"),
        "kD" => Some("kE"),
        "kQ" => Some("kAPOSTROPHE"),
        "kE" => Some("kPERIOD"),
        "kU" => Some("kF"),
        "kF" => Some("kU"),
        "kR" => Some("kP"),
        _ => None,
    }
}

#[derive(Debug, Clone)]
enum LtxValue {
    Plain(String),
    Bind(BindTable),
}

#[derive(Debug, Clone, Default)]
pub struct UserLtx {
    content: Vec<(String, LtxValue)>,
    file: Option<PathBuf>,
}

impl UserLtx {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(file: impl Into<PathBuf>) -> Result<Self> {
        let mut ltx = Self::new();
        ltx.load(file.into())?;
        Ok(ltx)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.content.iter().find(|(k, _)| k == key).and_then(|(_, v)| match v {
            LtxValue::Plain(s) => Some(s.as_str()),
            LtxValue::Bind(_) => None,
        })
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        if let Some(entry) = self.content.iter_mut().find(|(k, _)| k == key) {
            entry.1 = LtxValue::Plain(value);
        } else {
            self.content.push((key.to_string(), LtxValue::Plain(value)));
        }
    }

    fn bind_table(&mut self, key: &str) -> &mut BindTable {
        if !self.content.iter().any(|(k, _)| k == key) {
            self.content
                .push((key.to_string(), LtxValue::Bind(BindTable::new(key))));
        }

        match &mut self.content.iter_mut().find(|(k, _)| k == key).unwrap().1 {
            LtxValue::Bind(b) => b,
            LtxValue::Plain(_) => unreachable!(),
        }
    }

    pub fn bind(&mut self) -> &mut BindTable {
        self.bind_table("bind")
    }

    pub fn bind_sec(&mut self) -> &mut BindTable {
        self.bind_table("bind_sec")
    }

    pub fn load(&mut self, file: PathBuf) -> Result<()> {
        let text = std::fs::read_to_string(&file)?;

        for line in text.split('\n') {
            if line.is_empty() {
                continue;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let mut parts = trimmed.split(' ');
            let key = match parts.next() {
                Some(k) => k,
                None => continue,
            };
            let args: Vec<&str> = parts.collect();

            if key.contains("bind") {
                let sub_key = args.first().copied().unwrap_or("");
                let sub_value = if args.len() > 1 { args[1..].join(" ") } else { String::new() };
                self.bind_table(key).set(sub_key, sub_value);
            } else {
                self.set(key, args.join(" "));
            }
        }

        self.file = Some(file);
        Ok(())
    }

    pub fn save(&self, file: Option<&Path>) -> Result<()> {
        let target = file
            .map(|p| p.to_path_buf())
            .or_else(|| self.file.clone())
            .ok_or_else(|| {
                LauncherError::Other("file output need to be defined before save()".to_string())
            })?;

        let mut data = String::new();
        for (key, value) in &self.content {
            match value {
                LtxValue::Bind(b) => {
                    for (k, v) in &b.entries {
                        data.push_str(&format!("{} {} {}\r\n", b.kind, k, v));
                    }
                }
                LtxValue::Plain(v) if v.is_empty() => {
                    data.push_str(&format!("{key}\r\n"));
                }
                LtxValue::Plain(v) => {
                    data.push_str(&format!("{key} {v}\r\n"));
                }
            }
        }

        std::fs::write(target, data)?;
        Ok(())
    }
}
