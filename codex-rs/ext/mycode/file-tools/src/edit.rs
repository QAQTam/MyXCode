use codex_apply_patch::ApplyPatchFileUpdateMode;
use codex_apply_patch::seek_sequence::seek_sequence;
use similar::TextDiff;
use thiserror::Error;

/// Note appended to model-facing output when whitespace-tolerant matching was used.
pub const FUZZY_MATCH_NOTE: &str =
    " Note: old_string matched only after ignoring whitespace and Unicode punctuation differences.";

/// Number of context lines included in the generated unified diff.
const DIFF_CONTEXT_LINES: usize = 1;

/// A precise single-file replacement request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditRequest {
    pub old_string: String,
    pub new_string: String,
    pub replace_all: bool,
}

/// Which matching strategy produced the edit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditMatch {
    Exact,
    Fuzzy,
}

/// A validated, in-memory edit ready to be committed by the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditPlan {
    pub updated: String,
    pub replacements: usize,
    pub match_kind: EditMatch,
    pub unified_diff: String,
    pub message: String,
}

/// Validation and matching failures for a single-file edit.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum EditError {
    #[error("old_string must not be empty; use apply_patch to create a new file")]
    EmptyOldString,
    #[error("old_string and new_string are identical; nothing to change")]
    IdenticalStrings,
    #[error(
        "old_string appears {matches} times in the file. Add surrounding context to make it unique, or set replace_all to true."
    )]
    AmbiguousExact { matches: usize },
    #[error(
        "old_string matches {matches}+ locations in the file. Add surrounding context to make it unique, or set replace_all to true."
    )]
    AmbiguousFuzzy { matches: usize },
    #[error(
        "old_string was not found in the file. Read the file and copy the exact text, including whitespace."
    )]
    NotFound,
}

/// Plans a single-file replacement without performing any filesystem I/O.
pub fn plan_edit(
    original: &str,
    request: &EditRequest,
    update_file_mode: ApplyPatchFileUpdateMode,
) -> Result<EditPlan, EditError> {
    if request.old_string.is_empty() {
        return Err(EditError::EmptyOldString);
    }
    if request.old_string == request.new_string {
        return Err(EditError::IdenticalStrings);
    }

    let exact_matches = original.matches(&request.old_string).count();
    if exact_matches == 1 {
        let updated = original.replacen(&request.old_string, &request.new_string, 1);
        return Ok(plan_from_updated(
            original,
            updated,
            /*replacements*/ 1,
            EditMatch::Exact,
            "The file has been updated.".to_string(),
        ));
    }
    if exact_matches > 1 {
        if !request.replace_all {
            return Err(EditError::AmbiguousExact {
                matches: exact_matches,
            });
        }
        let updated = original.replace(&request.old_string, &request.new_string);
        return Ok(plan_from_updated(
            original,
            updated,
            exact_matches,
            EditMatch::Exact,
            format!("The file has been updated ({exact_matches} occurrences replaced)."),
        ));
    }

    let original_lines = split_lines(original);
    let pattern_lines = split_lines(&request.old_string);
    let line_offsets = line_offsets(original);

    let mut starts = Vec::new();
    let mut search_from = 0usize;
    while let Some(index) = seek_sequence(
        &original_lines,
        &pattern_lines,
        search_from,
        /*eof*/ false,
        update_file_mode,
    ) {
        starts.push(index);
        search_from = index + 1;
        if starts.len() > 1 && !request.replace_all {
            break;
        }
    }

    if starts.is_empty() {
        return Err(EditError::NotFound);
    }
    if starts.len() > 1 && !request.replace_all {
        return Err(EditError::AmbiguousFuzzy {
            matches: starts.len(),
        });
    }

    let mut updated = original.to_string();
    for start in starts.iter().rev() {
        let range = line_offsets[*start]..line_offsets[start + pattern_lines.len()];
        let mut replacement = request.new_string.clone();
        match (
            original[range.clone()].ends_with('\n'),
            replacement.ends_with('\n'),
        ) {
            (true, false) => replacement.push('\n'),
            (false, true) => {
                replacement.pop();
            }
            _ => {}
        }
        updated.replace_range(range, &replacement);
    }

    Ok(plan_from_updated(
        original,
        updated,
        starts.len(),
        EditMatch::Fuzzy,
        format!(
            "The file has been updated ({} occurrence(s) replaced).{FUZZY_MATCH_NOTE}",
            starts.len()
        ),
    ))
}

fn plan_from_updated(
    original: &str,
    updated: String,
    replacements: usize,
    match_kind: EditMatch,
    message: String,
) -> EditPlan {
    let unified_diff = TextDiff::from_lines(original, &updated)
        .unified_diff()
        .context_radius(DIFF_CONTEXT_LINES)
        .to_string();
    EditPlan {
        updated,
        replacements,
        match_kind,
        unified_diff,
        message,
    }
}

fn split_lines(contents: &str) -> Vec<String> {
    let mut lines = contents.split('\n').map(String::from).collect::<Vec<_>>();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn line_offsets(contents: &str) -> Vec<usize> {
    let mut offsets = vec![0usize];
    let mut offset = 0usize;
    for line in contents.split_inclusive('\n') {
        offset += line.len();
        offsets.push(offset);
    }
    offsets
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
