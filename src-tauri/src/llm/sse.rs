//! Incremental Server-Sent Events parser (used by every provider).

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Feed raw bytes as they arrive; complete events come out.
#[derive(Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
}

impl SseParser {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(newline) = self.buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=newline).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if let Some(event) = self.handle_line(line) {
                events.push(event);
            }
        }
        events
    }

    /// Flushes a trailing event that was not followed by a blank line.
    pub fn finish(&mut self) -> Option<SseEvent> {
        if !self.buffer.is_empty() {
            let rest = std::mem::take(&mut self.buffer);
            let line = String::from_utf8_lossy(&rest)
                .trim_end_matches('\r')
                .to_string();
            if let Some(event) = self.handle_line(&line) {
                return Some(event);
            }
        }
        self.dispatch()
    }

    fn handle_line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            return None; // comment / keep-alive
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => self.event = Some(value.to_string()),
            "data" => self.data.push(value.to_string()),
            _ => {} // id, retry: not needed
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if self.data.is_empty() {
            self.event = None;
            return None;
        }
        Some(SseEvent {
            event: self.event.take(),
            data: std::mem::take(&mut self.data).join("\n"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_split_across_chunks() {
        let mut parser = SseParser::default();
        assert!(parser.push(b"event: message_start\nda").is_empty());
        let events = parser.push(b"ta: {\"a\":1}\n\ndata: second\r\n\r\n");
        assert_eq!(
            events,
            vec![
                SseEvent {
                    event: Some("message_start".into()),
                    data: "{\"a\":1}".into()
                },
                SseEvent {
                    event: None,
                    data: "second".into()
                },
            ]
        );
    }

    #[test]
    fn joins_multiline_data_and_ignores_comments() {
        let mut parser = SseParser::default();
        let events = parser.push(b": keep-alive\ndata: line one\ndata: line two\n\n");
        assert_eq!(events[0].data, "line one\nline two");
    }

    #[test]
    fn keeps_multibyte_characters_split_between_chunks() {
        let mut parser = SseParser::default();
        let bytes = "data: Grüße\n\n".as_bytes();
        let split = bytes.iter().position(|&b| b == 0xC3).unwrap() + 1; // inside 'ü'
        assert!(parser.push(&bytes[..split]).is_empty());
        assert_eq!(parser.push(&bytes[split..])[0].data, "Grüße");
    }

    #[test]
    fn flushes_unterminated_final_event() {
        let mut parser = SseParser::default();
        assert!(parser.push(b"data: [DONE]").is_empty());
        assert_eq!(parser.finish().unwrap().data, "[DONE]");
    }
}
