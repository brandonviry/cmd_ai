//! SSE line parser.
//!
//! The subset of server-sent events used by OpenAI-compatible servers: `data:` fields
//! (with or without the space after the colon), multi-line data joined with `\n`, a
//! blank line dispatches the event, lines starting with `:` are comments, other fields
//! (`event`, `id`, `retry`) are ignored, and `data: [DONE]` ends the stream. Line
//! terminators (`\n`, `\r\n`, `\r`) are tolerated.

/// End-of-stream marker of OpenAI-compatible servers.
const DONE_MARKER: &str = "[DONE]";

/// One dispatched server-sent event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    /// The `data` of one event (lines joined with `\n`).
    Data(String),
    /// `data: [DONE]`: nothing follows.
    Done,
}

/// Incremental parser, fed one line at a time.
#[derive(Debug, Default)]
pub struct SseParser {
    data: String,
    has_data: bool,
    started: bool,
    done: bool,
}

impl SseParser {
    pub fn new() -> Self {
        SseParser::default()
    }

    /// Feeds one line (its terminator may be included). Returns an event when this
    /// line completes one.
    pub fn push_line(&mut self, line: &str) -> Option<SseEvent> {
        if self.done {
            return None;
        }
        let mut line = line.strip_suffix('\n').unwrap_or(line);
        line = line.strip_suffix('\r').unwrap_or(line);
        if !self.started {
            self.started = true;
            line = line.strip_prefix('\u{feff}').unwrap_or(line);
        }
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        if field != "data" {
            return None;
        }
        if !self.has_data && value == DONE_MARKER {
            // Some servers close right after the marker, without the blank line.
            self.done = true;
            return Some(SseEvent::Done);
        }
        if self.has_data {
            self.data.push('\n');
        }
        self.data.push_str(value);
        self.has_data = true;
        None
    }

    /// End of input: returns the pending event, if any (stream closed without the
    /// final blank line).
    pub fn finish(&mut self) -> Option<SseEvent> {
        if self.done {
            return None;
        }
        self.dispatch()
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if !self.has_data {
            return None;
        }
        self.has_data = false;
        let data = std::mem::take(&mut self.data);
        if data == DONE_MARKER {
            self.done = true;
            return Some(SseEvent::Done);
        }
        Some(SseEvent::Data(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(lines: &[&str]) -> Vec<SseEvent> {
        let mut p = SseParser::new();
        lines.iter().filter_map(|l| p.push_line(l)).collect()
    }

    fn data(s: &str) -> SseEvent {
        SseEvent::Data(s.to_string())
    }

    #[test]
    fn blank_line_dispatches_event() {
        let mut p = SseParser::new();
        assert_eq!(p.push_line(r#"data: {"a":1}"#), None);
        assert_eq!(p.push_line(""), Some(data(r#"{"a":1}"#)));
    }

    #[test]
    fn data_without_space_after_colon() {
        assert_eq!(feed(&["data:{\"a\":1}", ""]), vec![data("{\"a\":1}")]);
    }

    #[test]
    fn only_one_leading_space_is_removed() {
        assert_eq!(feed(&["data:  x", ""]), vec![data(" x")]);
        assert_eq!(feed(&["data: x ", ""]), vec![data("x ")]);
    }

    #[test]
    fn multi_line_data_is_joined_with_newline() {
        assert_eq!(
            feed(&["data: line1", "data:line2", "data: ", ""]),
            vec![data("line1\nline2\n")]
        );
    }

    #[test]
    fn comments_are_ignored() {
        assert_eq!(feed(&[": keep-alive", ""]), vec![]);
        assert_eq!(
            feed(&[":", "data: a", ": ping", "data: b", ""]),
            vec![data("a\nb")]
        );
    }

    #[test]
    fn done_marker_gives_done() {
        assert_eq!(feed(&["data: [DONE]", ""]), vec![SseEvent::Done]);
        assert_eq!(
            feed(&["data:[DONE]"]),
            vec![SseEvent::Done],
            "no blank line needed"
        );
        assert_eq!(feed(&["data: [DONE]\r\n", "\r\n"]), vec![SseEvent::Done]);
    }

    #[test]
    fn nothing_after_done() {
        assert_eq!(
            feed(&["data: [DONE]", "", "data: late", ""]),
            vec![SseEvent::Done]
        );
    }

    #[test]
    fn done_inside_multi_line_data_is_plain_data() {
        assert_eq!(
            feed(&["data: x", "data: [DONE]", ""]),
            vec![data("x\n[DONE]")]
        );
    }

    #[test]
    fn crlf_and_lf_terminators_are_tolerated() {
        assert_eq!(feed(&["data: x\r\n", "\r\n"]), vec![data("x")]);
        assert_eq!(feed(&["data: y\n", "\n"]), vec![data("y")]);
        assert_eq!(feed(&["data: z\r", "\r"]), vec![data("z")]);
        assert_eq!(feed(&[": c\r\n", "data: w\r\n", "\n"]), vec![data("w")]);
    }

    #[test]
    fn blank_lines_without_data_do_not_dispatch() {
        assert_eq!(feed(&["", "", "\r\n"]), vec![]);
        assert_eq!(feed(&["event: ping", ""]), vec![]);
    }

    #[test]
    fn other_fields_are_ignored() {
        assert_eq!(
            feed(&["event: message", "id: 42", "retry: 1000", "data: v", ""]),
            vec![data("v")]
        );
    }

    #[test]
    fn field_names_are_case_sensitive() {
        assert_eq!(feed(&["Data: v", ""]), vec![]);
    }

    #[test]
    fn data_field_without_colon_is_empty_data() {
        assert_eq!(feed(&["data", ""]), vec![data("")]);
    }

    #[test]
    fn consecutive_events_are_independent() {
        assert_eq!(
            feed(&["data: 1", "", "data: 2", "", "", "data: 3", ""]),
            vec![data("1"), data("2"), data("3")]
        );
    }

    #[test]
    fn finish_flushes_pending_data() {
        let mut p = SseParser::new();
        assert_eq!(p.push_line("data: tail"), None);
        assert_eq!(p.finish(), Some(data("tail")));
        assert_eq!(p.finish(), None);
        let mut q = SseParser::new();
        assert_eq!(q.finish(), None);
    }

    #[test]
    fn finish_after_done_is_none() {
        let mut p = SseParser::new();
        assert_eq!(p.push_line("data: [DONE]"), Some(SseEvent::Done));
        assert_eq!(p.finish(), None);
    }

    #[test]
    fn byte_order_mark_on_first_line_is_skipped() {
        assert_eq!(feed(&["\u{feff}data: x", ""]), vec![data("x")]);
        assert_eq!(feed(&["\u{feff}: comment", "data: y", ""]), vec![data("y")]);
    }

    #[test]
    fn unicode_and_colons_in_value_are_preserved() {
        assert_eq!(
            feed(&["data: {\"t\":\"é: à ✓\"}", ""]),
            vec![data("{\"t\":\"é: à ✓\"}")]
        );
    }

    #[test]
    fn default_is_same_as_new() {
        let mut p = SseParser::default();
        assert_eq!(p.push_line("data: [DONE]"), Some(SseEvent::Done));
    }
}
