use super::*;
use codex_apply_patch::ApplyPatchFileUpdateMode;
use pretty_assertions::assert_eq;

const UPDATE_MODE: ApplyPatchFileUpdateMode = ApplyPatchFileUpdateMode::NormalizeToLf;

fn edit(
    original: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<EditPlan, EditError> {
    plan_edit(
        original,
        &EditRequest {
            old_string: old_string.to_string(),
            new_string: new_string.to_string(),
            replace_all,
        },
        UPDATE_MODE,
    )
}

#[test]
fn exact_match_replaces_a_single_occurrence() {
    let outcome = edit(
        "alpha\nbeta\ngamma\n",
        "beta",
        "BETA",
        /*replace_all*/ false,
    )
    .expect("exact edit should succeed");

    assert_eq!(outcome.updated, "alpha\nBETA\ngamma\n");
    assert_eq!(outcome.message, "The file has been updated.");
    assert_eq!(outcome.match_kind, EditMatch::Exact);
}

#[test]
fn repeated_old_string_requires_context_or_replace_all() {
    let error =
        edit("a\na\n", "a", "b", /*replace_all*/ false).expect_err("ambiguous edit must fail");
    assert_eq!(error, EditError::AmbiguousExact { matches: 2 });

    let outcome = edit("a\na\n", "a", "b", /*replace_all*/ true).expect("replace_all should work");
    assert_eq!(outcome.updated, "b\nb\n");
    assert_eq!(
        outcome.message,
        "The file has been updated (2 occurrences replaced)."
    );
}

#[test]
fn missing_old_string_reports_not_found() {
    let error = edit("alpha\n", "omega", "x", /*replace_all*/ false)
        .expect_err("missing old_string must fail");
    assert_eq!(error, EditError::NotFound);
}

#[test]
fn trailing_whitespace_difference_falls_back_to_fuzzy_match() {
    let outcome = edit(
        "fn main() {\n    let x = 1;   \n}\n",
        "fn main() {\n    let x = 1;\n}",
        "fn main() {\n    let x = 2;\n}",
        /*replace_all*/ false,
    )
    .expect("fuzzy edit should succeed");

    assert_eq!(outcome.updated, "fn main() {\n    let x = 2;\n}\n");
    assert_eq!(outcome.match_kind, EditMatch::Fuzzy);
    assert!(
        outcome.message.ends_with(FUZZY_MATCH_NOTE),
        "{}",
        outcome.message
    );
}

#[test]
fn exact_match_inside_a_longer_line_is_not_line_scoped() {
    let outcome = edit(
        "head\n    keep me   \ntail\n",
        "keep me",
        "KEEP ME",
        /*replace_all*/ false,
    )
    .expect("exact substring edit should succeed");

    assert_eq!(outcome.updated, "head\n    KEEP ME   \ntail\n");
    assert_eq!(outcome.message, "The file has been updated.");
}

#[test]
fn fuzzy_match_replaces_whole_lines_and_keeps_the_trailing_newline() {
    let outcome = edit(
        "head\nkeep me   \ntail\n",
        "keep me\n",
        "KEEP ME\n",
        /*replace_all*/ false,
    )
    .expect("fuzzy edit should succeed");

    assert_eq!(outcome.updated, "head\nKEEP ME\ntail\n");
    assert_eq!(outcome.match_kind, EditMatch::Fuzzy);
}

#[test]
fn ambiguous_fuzzy_match_requires_replace_all() {
    let original = "one   \ntwo\none   \n";
    let old_string = "one  \n";

    let error = edit(original, old_string, "1", /*replace_all*/ false)
        .expect_err("ambiguous fuzzy match must fail");
    assert_eq!(error, EditError::AmbiguousFuzzy { matches: 2 });

    let outcome =
        edit(original, old_string, "1", /*replace_all*/ true).expect("fuzzy replace_all works");
    assert_eq!(outcome.updated, "1\ntwo\n1\n");
}

#[test]
fn line_offsets_cover_every_line_terminator() {
    assert_eq!(line_offsets(""), vec![0]);
    assert_eq!(line_offsets("a\nb"), vec![0, 2, 3]);
    assert_eq!(line_offsets("a\nb\n"), vec![0, 2, 4]);
    assert_eq!(
        split_lines("a\nb\n"),
        vec!["a".to_string(), "b".to_string()]
    );
}

#[test]
fn rejects_empty_and_identical_requests() {
    assert_eq!(
        edit("alpha\n", "", "x", /*replace_all*/ false),
        Err(EditError::EmptyOldString)
    );
    assert_eq!(
        edit("alpha\n", "alpha", "alpha", /*replace_all*/ false),
        Err(EditError::IdenticalStrings)
    );
}
