//! Pure plan for the create-only `write_file` tool.

/// A validated request to create a new file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WritePlan {
    pub content: String,
}
