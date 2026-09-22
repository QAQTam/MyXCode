use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

/// Creates the JSON function spec for precise single-file edits.
pub fn create_edit_file_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "file_path".to_string(),
            JsonSchema::string(Some("Absolute path to the file to modify.".to_string())),
        ),
        (
            "old_string".to_string(),
            JsonSchema::string(Some(
                "The exact text to replace, copied verbatim from the file (or from read output with the `NNN→` line-number prefix removed). Must be unique in the file unless replace_all is true.".to_string(),
            )),
        ),
        (
            "new_string".to_string(),
            JsonSchema::string(Some(
                "The text to replace it with. Must be different from old_string.".to_string(),
            )),
        ),
        (
            "replace_all".to_string(),
            JsonSchema::boolean(Some(
                "Replace all occurrences of old_string. Defaults to false.".to_string(),
            )),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "edit_file".to_string(),
        description: "Performs exact string replacement in a single file. Prefer this over apply_patch for single-file precise edits. Read the file first with the read_file tool and copy the exact text; read_file's `NNN→` line-number prefix is not part of the file, so strip it from old_string and new_string. The edit fails if old_string is not unique (pass more context or set replace_all).".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec![
                "file_path".to_string(),
                "old_string".to_string(),
                "new_string".to_string(),
            ]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}

/// Creates the JSON function spec for bounded, line-numbered text reads.
pub fn create_read_file_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "file_path".to_string(),
            JsonSchema::string(Some("Absolute path to the file to read.".to_string())),
        ),
        (
            "offset".to_string(),
            JsonSchema::integer(Some(
                "1-based line number to start reading from. Omit to start at line 1.".to_string(),
            )),
        ),
        (
            "limit".to_string(),
            JsonSchema::integer(Some(
                "Maximum number of lines to return. Omit to use the default. Use offset + limit to read a file in chunks when it is too large to read at once.".to_string(),
            )),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "read_file".to_string(),
        description: "Reads a text file from the local filesystem. Returns lines in `cat -n` format: a right-aligned line number, then `→`, then the line's exact text. Line numbers are for reference only — never include the `NNN→` prefix in edit_file.old_string or new_string. Reads are bounded: if the requested range is truncated, the output ends with a note telling you the next offset to continue from. Non-UTF-8 files (images, binaries) are rejected; use view_image for images.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["file_path".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}

/// Creates the JSON function spec for create-only file writes.
pub fn create_write_file_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "file_path".to_string(),
            JsonSchema::string(Some(
                "Absolute path to the new file to create. Parent directories are created automatically.".to_string(),
            )),
        ),
        (
            "content".to_string(),
            JsonSchema::string(Some("The full content of the new file.".to_string())),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "write_file".to_string(),
        description: "Creates a new file with the given content. Parent directories are created automatically. This tool creates files only: it fails if the path already exists. To change an existing file, use edit_file for targeted replacements, or apply_patch for a full rewrite.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["file_path".to_string(), "content".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}
