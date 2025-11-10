use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LstNote {
    pub title: String,
    pub content: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    #[serde(default)]
    pub metadata: NoteMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NoteMetadata {
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteMatch {
    pub note: String,
    pub line: usize,
    pub content: String,
    #[serde(default)]
    pub context: Vec<String>,
}

pub struct LstIntegration {
    pub available: bool,
}

impl LstIntegration {
    pub fn new() -> Self {
        let available = Command::new("lst").arg("--version").output().is_ok();

        Self { available }
    }

    pub fn check_available(&self) -> crate::Result<()> {
        if !self.available {
            return Err(crate::Error::Integration(
                "lst is not installed or not in PATH".to_string(),
            ));
        }
        Ok(())
    }

    /// Get note content using lst note show --json
    pub async fn get_note(&self, note_name: &str) -> crate::Result<LstNote> {
        self.check_available()?;

        let output = Command::new("lst")
            .args(["note", "show", note_name, "--json"])
            .output()?;

        if !output.status.success() {
            return Err(crate::Error::Integration(format!(
                "Failed to get note '{}': {}",
                note_name,
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let note: LstNote = serde_json::from_slice(&output.stdout)?;
        Ok(note)
    }

    /// Search within lst notes using lst note grep --json
    pub async fn search_notes(&self, pattern: &str) -> crate::Result<Vec<NoteMatch>> {
        self.check_available()?;

        let output = Command::new("lst")
            .args(["note", "grep", pattern, "--json"])
            .output()?;

        if !output.status.success() {
            return Err(crate::Error::Integration(format!(
                "Failed to search notes: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let matches: Vec<NoteMatch> = serde_json::from_slice(&output.stdout)?;
        Ok(matches)
    }

    /// Add text to a note using lst note add
    pub async fn add_to_note(&self, note_name: &str, content: &str) -> crate::Result<()> {
        self.check_available()?;

        let output = Command::new("lst")
            .args(["note", "add", note_name, content])
            .output()?;

        if !output.status.success() {
            return Err(crate::Error::Integration(format!(
                "Failed to add to note '{}': {}",
                note_name,
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    /// Add item to a list using lst add
    pub async fn add_to_list(&self, list_name: &str, item: &str) -> crate::Result<()> {
        self.check_available()?;

        let output = Command::new("lst")
            .args(["add", list_name, item])
            .output()?;

        if !output.status.success() {
            return Err(crate::Error::Integration(format!(
                "Failed to add to list '{}': {}",
                list_name,
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }
}

impl Default for LstIntegration {
    fn default() -> Self {
        Self::new()
    }
}
