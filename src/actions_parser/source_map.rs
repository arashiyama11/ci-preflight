use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Source {
    Yaml {
        path: PathBuf,
        key: String,
        text: String,
    },
    ShFile {
        path: PathBuf,
        text: String,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceId(pub u32);
pub struct SourceMap {
    docs: Vec<Source>,
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap { docs: vec![] }
    }

    pub fn add_yaml(&mut self, path: PathBuf, key: String, text: String) -> SourceId {
        let id = self.docs.len() as u32;
        self.docs.push(Source::Yaml { path, key, text });
        SourceId(id)
    }

    pub fn add_sh_file(&mut self, path: PathBuf, text: String) -> SourceId {
        let id = self.docs.len() as u32;
        self.docs.push(Source::ShFile { path, text });
        SourceId(id)
    }

    pub fn get_text(&self, id: &SourceId) -> Option<&str> {
        match self.docs.get(id.0 as usize)? {
            Source::Yaml { text, .. } => Some(text.as_str()),
            Source::ShFile { text, .. } => Some(text.as_str()),
        }
    }
}
