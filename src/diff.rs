use facet::Facet;

#[derive(Clone, Debug, Facet, PartialEq)]
#[repr(C)]
#[facet(rename_all = "snake_case")]
pub enum ReviewTarget {
    WorkingTree,
    Cached,
    All,
    Branch { base: String, head: String },
}

impl ReviewTarget {
    pub fn label(&self) -> String {
        match self {
            Self::WorkingTree => "working tree".to_string(),
            Self::Cached => "cached".to_string(),
            Self::All => "all current changes".to_string(),
            Self::Branch { base, head } => format!("{base}..{head}"),
        }
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
#[repr(u8)]
#[facet(rename_all = "snake_case")]
pub enum FileSide {
    Old,
    New,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct LineAnchor {
    pub path: String,
    pub old_path: Option<String>,
    pub side: FileSide,
    pub start_line: u32,
    pub end_line: u32,
    pub hunk_header: Option<String>,
    pub selected_text_hash: Option<String>,
    pub nearby_context_hash: Option<String>,
    pub base_oid: Option<String>,
    pub head_oid: Option<String>,
}

impl LineAnchor {
    pub fn new(path: String, side: FileSide, start_line: u32, end_line: u32) -> Self {
        Self {
            path,
            old_path: None,
            side,
            start_line,
            end_line,
            hunk_header: None,
            selected_text_hash: None,
            nearby_context_hash: None,
            base_oid: None,
            head_oid: None,
        }
    }

    pub fn line_label(&self) -> String {
        if self.start_line == self.end_line {
            format!("{}:{}", self.path, self.start_line)
        } else {
            format!("{}:{}-{}", self.path, self.start_line, self.end_line)
        }
    }
}
