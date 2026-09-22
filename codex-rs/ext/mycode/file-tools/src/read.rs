//! Bounded, line-numbered text scanning shared by the `read_file` handler.
//!
//! The scanner is deliberately independent from filesystem I/O. Core feeds it bytes from the
//! selected environment filesystem, then renders the resulting line window.

/// Lines returned when the caller does not pass `limit`.
pub const DEFAULT_MAX_LINES: usize = 500;

/// Longest single line that can be shown. Longer lines are a hard error.
pub const MAX_LINE_BYTES: usize = 32 * 1024;

/// Total rendered output budget, counted in bytes of what the model receives.
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;

const PREFIX_WIDTH: usize = 6;
const LINE_ARROW: &str = "→";
const EMPTY_FILE_NOTE: &str = "[READ EMPTY: the file exists but has no content]";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadStop {
    LineLimit,
    ByteBudget,
}

impl ReadStop {
    fn reason(self) -> &'static str {
        match self {
            ReadStop::LineLimit => "line limit reached",
            ReadStop::ByteBudget => "output byte budget reached",
        }
    }
}

/// A line-level failure that aborts the read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadError {
    LineTooLong { line: usize, bytes: usize },
    NotUtf8 { line: usize },
}

impl ReadError {
    /// Converts a scanner failure into the model-facing error used by `read_file`.
    pub fn model_message(self, path: &str) -> String {
        match self {
            ReadError::NotUtf8 { line } => format!(
                "`{path}` is not valid UTF-8 text (first invalid line: {line}); it may be a binary file. Use view_image for images, or exec to inspect binary content."
            ),
            ReadError::LineTooLong { line, bytes } => format!(
                "line {line} of `{path}` is {bytes} bytes, which exceeds the {MAX_LINE_BYTES}-byte per-line limit; use exec to inspect or split it."
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NumberedLine {
    number: usize,
    text: String,
}

impl NumberedLine {
    fn rendered_len(&self) -> usize {
        prefix_len(self.number) + self.text.len() + 1
    }
}

fn prefix_len(number: usize) -> usize {
    number.to_string().len().max(PREFIX_WIDTH) + LINE_ARROW.len()
}

/// Result of scanning a (possibly partial) file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadOutcome {
    lines: Vec<NumberedLine>,
    stop: Option<ReadStop>,
    lines_seen: usize,
}

/// Incremental, byte-oriented line scanner with window and byte-budget gates.
#[derive(Debug)]
pub struct ReadScanner {
    first_line: usize,
    max_lines: usize,
    max_line_bytes: usize,
    max_output_bytes: usize,
    line_index: usize,
    partial: Vec<u8>,
    partial_len: usize,
    lines_seen: usize,
    selected: Vec<NumberedLine>,
    output_bytes: usize,
    stop: Option<ReadStop>,
}

impl ReadScanner {
    /// Creates a scanner using the production line, per-line, and output limits.
    pub fn new(offset: Option<usize>, limit: Option<usize>) -> Self {
        let requested_offset = offset.unwrap_or(1).max(1);
        let max_lines = match limit {
            None | Some(0) => DEFAULT_MAX_LINES,
            Some(limit) => limit,
        };
        Self::new_with_limits(
            requested_offset - 1,
            max_lines,
            MAX_LINE_BYTES,
            MAX_OUTPUT_BYTES,
        )
    }

    fn new_with_limits(
        first_line: usize,
        max_lines: usize,
        max_line_bytes: usize,
        max_output_bytes: usize,
    ) -> Self {
        Self {
            first_line,
            max_lines,
            max_line_bytes,
            max_output_bytes,
            line_index: 0,
            partial: Vec::new(),
            partial_len: 0,
            lines_seen: 0,
            selected: Vec::new(),
            output_bytes: 0,
            stop: None,
        }
    }

    /// Returns whether the scanner already has all output it can produce.
    pub fn is_stopped(&self) -> bool {
        self.stop.is_some()
    }

    /// Feeds one chunk. Bytes after a stop decision are ignored.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<(), ReadError> {
        if self.stop.is_some() {
            return Ok(());
        }
        let mut rest = chunk;
        while let Some(position) = rest.iter().position(|byte| *byte == b'\n') {
            self.append_partial(&rest[..position]);
            rest = &rest[position + 1..];
            self.finish_line()?;
            if self.stop.is_some() {
                return Ok(());
            }
        }
        self.append_partial(rest);
        Ok(())
    }

    /// Closes the scan, treating a non-empty remainder as a final unterminated line.
    pub fn finish(mut self) -> Result<ReadOutcome, ReadError> {
        if self.stop.is_none() && self.partial_len > 0 {
            self.finish_line()?;
        }
        Ok(ReadOutcome {
            lines: self.selected,
            stop: self.stop,
            lines_seen: self.lines_seen,
        })
    }

    fn append_partial(&mut self, bytes: &[u8]) {
        self.partial_len += bytes.len();
        let cap = self.max_line_bytes + 1;
        if self.partial.len() < cap {
            let take = (cap - self.partial.len()).min(bytes.len());
            self.partial.extend_from_slice(&bytes[..take]);
        }
    }

    fn finish_line(&mut self) -> Result<(), ReadError> {
        let index = self.line_index;
        self.line_index += 1;
        self.lines_seen += 1;

        if index < self.first_line {
            self.partial.clear();
            self.partial_len = 0;
            return Ok(());
        }
        if self.partial_len > self.max_line_bytes {
            return Err(ReadError::LineTooLong {
                line: index + 1,
                bytes: self.partial_len,
            });
        }

        let mut text = String::from_utf8(std::mem::take(&mut self.partial))
            .map_err(|_| ReadError::NotUtf8 { line: index + 1 })?;
        self.partial_len = 0;
        if text.ends_with('\r') {
            text.pop();
        }
        if index == 0 && text.starts_with('\u{feff}') {
            text.replace_range(..'\u{feff}'.len_utf8(), "");
        }

        let number = index + 1;
        let line = NumberedLine { number, text };
        if self.selected.is_empty()
            || self.output_bytes + line.rendered_len() <= self.max_output_bytes
        {
            self.output_bytes += line.rendered_len();
            self.selected.push(line);
        } else {
            self.stop = Some(ReadStop::ByteBudget);
            return Ok(());
        }
        if self.selected.len() >= self.max_lines {
            self.stop = Some(ReadStop::LineLimit);
        }
        Ok(())
    }
}

fn render_lines(lines: &[NumberedLine]) -> String {
    let mut rendered = String::new();
    for line in lines {
        if !rendered.is_empty() {
            rendered.push('\n');
        }
        let digits = line.number.to_string();
        for _ in digits.len()..PREFIX_WIDTH {
            rendered.push(' ');
        }
        rendered.push_str(&digits);
        rendered.push_str(LINE_ARROW);
        rendered.push_str(&line.text);
    }
    rendered
}

/// Renders the scanner outcome into the model-facing `cat -n` text.
pub fn render_read_output(
    outcome: &ReadOutcome,
    modified_at_ms: i64,
    requested_offset: usize,
) -> String {
    let body = render_lines(&outcome.lines);
    let Some(stop) = outcome.stop else {
        if !outcome.lines.is_empty() {
            return body;
        }
        if outcome.lines_seen == 0 {
            return EMPTY_FILE_NOTE.to_string();
        }
        return format!(
            "[READ EMPTY: offset {requested_offset} is past the end of the file, which has {} line(s)]",
            outcome.lines_seen
        );
    };

    let first = outcome
        .lines
        .first()
        .map_or(requested_offset, |line| line.number);
    let last = outcome
        .lines
        .last()
        .map_or(requested_offset, |line| line.number);
    let marker = format!(
        "[READ INCOMPLETE: showing lines {first}-{last} ({}); call read again with offset={} to continue. file_modified_at_ms={modified_at_ms}]",
        stop.reason(),
        last + 1,
    );
    if body.is_empty() {
        marker
    } else {
        format!("{body}\n{marker}")
    }
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
