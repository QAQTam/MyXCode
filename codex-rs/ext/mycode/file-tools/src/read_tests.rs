use super::*;
use pretty_assertions::assert_eq;

fn scan_with_budgets(
    chunks: &[&[u8]],
    first_line: usize,
    max_lines: usize,
    max_line_bytes: usize,
    max_output_bytes: usize,
) -> Result<ReadOutcome, ReadError> {
    let mut scanner =
        ReadScanner::new_with_limits(first_line, max_lines, max_line_bytes, max_output_bytes);
    for chunk in chunks {
        scanner.feed(chunk)?;
    }
    scanner.finish()
}

fn scan_str(contents: &str, first_line: usize, max_lines: usize) -> ReadOutcome {
    scan_with_budgets(
        &[contents.as_bytes()],
        first_line,
        max_lines,
        MAX_LINE_BYTES,
        MAX_OUTPUT_BYTES,
    )
    .expect("scan should succeed")
}

fn rendered(contents: &str) -> String {
    let outcome = scan_str(contents, 0, DEFAULT_MAX_LINES);
    render_lines(&outcome.lines)
}

#[test]
fn renders_cat_n_format_with_padded_line_numbers() {
    assert_eq!(rendered("alpha\nbeta\n"), "     1→alpha\n     2→beta");
}

#[test]
fn trailing_newline_does_not_create_a_phantom_line() {
    let with_newline = scan_str("a\n", 0, DEFAULT_MAX_LINES);
    let without_newline = scan_str("a", 0, DEFAULT_MAX_LINES);
    assert_eq!(with_newline.lines, without_newline.lines);
    assert_eq!(with_newline.lines_seen, 1);
}

#[test]
fn strips_crlf_and_leading_bom() {
    let outcome = scan_str("\u{feff}a\r\nb\r\n", 0, DEFAULT_MAX_LINES);
    assert_eq!(outcome.lines[0].text, "a");
    assert_eq!(outcome.lines[1].text, "b");
}

#[test]
fn empty_file_renders_an_explicit_note() {
    let outcome = scan_str("", 0, DEFAULT_MAX_LINES);
    assert_eq!(
        render_read_output(
            &outcome, /*modified_at_ms*/ 0, /*requested_offset*/ 1
        ),
        EMPTY_FILE_NOTE
    );
}

#[test]
fn offset_and_line_limit_keep_file_line_numbers() {
    let outcome = scan_str("a\nb\nc\nd\n", /*first_line*/ 1, /*max_lines*/ 2);
    assert_eq!(outcome.lines.len(), 2);
    assert_eq!(outcome.lines[0].number, 2);
    assert_eq!(outcome.lines[1].number, 3);
    assert_eq!(outcome.stop, Some(ReadStop::LineLimit));

    let text = render_read_output(&outcome, 1234, 2);
    assert!(text.contains("offset=4"), "{text}");
}

#[test]
fn byte_budget_stops_on_a_line_boundary() {
    let outcome = scan_with_budgets(
        &[b"one\ntwo\nthree\n"],
        0,
        DEFAULT_MAX_LINES,
        MAX_LINE_BYTES,
        /*max_output_bytes*/ 16,
    )
    .expect("scan");
    assert_eq!(outcome.stop, Some(ReadStop::ByteBudget));
    assert_eq!(outcome.lines.len(), 1);
    assert_eq!(outcome.lines[0].text, "one");
}

#[test]
fn long_line_is_a_hard_error() {
    let error = scan_with_budgets(
        &[b"12345\n"],
        0,
        DEFAULT_MAX_LINES,
        /*max_line_bytes*/ 4,
        MAX_OUTPUT_BYTES,
    )
    .expect_err("line should be rejected");
    assert_eq!(error, ReadError::LineTooLong { line: 1, bytes: 5 });
}

#[test]
fn invalid_utf8_reports_the_line() {
    let error = scan_with_budgets(
        &[b"ok\n\xff\n"],
        0,
        DEFAULT_MAX_LINES,
        MAX_LINE_BYTES,
        MAX_OUTPUT_BYTES,
    )
    .expect_err("invalid UTF-8 should fail");
    assert_eq!(error, ReadError::NotUtf8 { line: 2 });
}

#[test]
fn scanner_is_invariant_to_chunk_boundaries() {
    let bytes = "one\ntwo\nthree\n".as_bytes();
    let whole = scan_with_budgets(
        &[bytes],
        0,
        DEFAULT_MAX_LINES,
        MAX_LINE_BYTES,
        MAX_OUTPUT_BYTES,
    )
    .expect("whole scan");

    for split in 0..=bytes.len() {
        let (left, right) = bytes.split_at(split);
        let chunked = scan_with_budgets(
            &[left, right],
            0,
            DEFAULT_MAX_LINES,
            MAX_LINE_BYTES,
            MAX_OUTPUT_BYTES,
        )
        .expect("chunked scan");
        assert_eq!(chunked.lines, whole.lines, "split={split}");
        assert_eq!(chunked.stop, whole.stop, "split={split}");
    }
}
