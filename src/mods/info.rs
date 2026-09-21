#[derive(Debug, Clone, Default)]
pub struct ModInfo {
    pub author: String,
    pub name: String,
    pub title: String,
    pub url: String,
    pub iurl: String,
    pub subdirs: Option<Vec<String>>,
    pub args: Option<Vec<String>>,
}

impl ModInfo {
    pub fn from_url(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Default::default()
        }
    }

    pub fn moddb(name: impl Into<String>, url: impl Into<String>, iurl: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            iurl: iurl.into(),
            ..Default::default()
        }
    }

    pub fn separator(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            &self.name
        } else {
            &self.title
        }
    }

    pub fn display_byline(&self) -> String {
        let author = self.author.trim();
        if author.is_empty() {
            return self.display_title().to_string();
        }
        format!("{} by {author}", self.display_title())
    }
}
