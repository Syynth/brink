use std::time::{Duration, Instant};

/// Character-by-character text reveal state.
pub struct TypewriterState {
    full_text: String,
    revealed_chars: usize,
    total_chars: usize,
    last_tick: Instant,
    char_delay: Duration,
}

impl TypewriterState {
    pub fn new(text: String, char_delay: Duration) -> Self {
        let total_chars = text.chars().count();
        Self {
            full_text: text,
            revealed_chars: 0,
            total_chars,
            last_tick: Instant::now(),
            char_delay,
        }
    }

    /// Advance the reveal by elapsed time. Returns `true` if new characters
    /// were revealed this tick.
    pub fn tick(&mut self) -> bool {
        if self.is_complete() {
            return false;
        }

        let elapsed = self.last_tick.elapsed();
        if elapsed < self.char_delay {
            return false;
        }

        let chars_to_reveal =
            usize::try_from(elapsed.as_millis() / self.char_delay.as_millis().max(1))
                .unwrap_or(usize::MAX);
        let new_count = (self.revealed_chars + chars_to_reveal).min(self.total_chars);

        if new_count == self.revealed_chars {
            return false;
        }

        self.revealed_chars = new_count;
        self.last_tick = Instant::now();
        true
    }

    /// Skip to fully revealed.
    pub fn skip(&mut self) {
        self.revealed_chars = self.total_chars;
    }

    pub fn is_complete(&self) -> bool {
        self.revealed_chars >= self.total_chars
    }

    /// Return the full (unrevealed) text.
    pub fn full_text(&self) -> &str {
        &self.full_text
    }

    /// Return the currently visible portion of the text.
    pub fn visible_text(&self) -> &str {
        if self.revealed_chars >= self.total_chars {
            &self.full_text
        } else {
            let byte_end = self
                .full_text
                .char_indices()
                .nth(self.revealed_chars)
                .map_or(self.full_text.len(), |(i, _)| i);
            &self.full_text[..byte_end]
        }
    }

    /// Consume the typewriter and return the full text.
    pub fn into_text(self) -> String {
        self.full_text
    }

    /// How long to wait before the next tick is useful.
    pub fn poll_timeout(&self) -> Duration {
        if self.is_complete() {
            Duration::from_millis(100)
        } else {
            let since = self.last_tick.elapsed();
            self.char_delay.saturating_sub(since)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_text_starts_empty() {
        let tw = TypewriterState::new("Hello".to_owned(), Duration::from_millis(50));
        assert_eq!(tw.visible_text(), "");
        assert!(!tw.is_complete());
    }

    #[test]
    fn skip_reveals_all() {
        let mut tw = TypewriterState::new("Hello".to_owned(), Duration::from_millis(50));
        tw.skip();
        assert_eq!(tw.visible_text(), "Hello");
        assert!(tw.is_complete());
    }

    #[test]
    fn into_text_returns_full() {
        let tw = TypewriterState::new("Hello".to_owned(), Duration::from_millis(50));
        assert_eq!(tw.into_text(), "Hello");
    }

    #[test]
    fn full_text_available_during_reveal() {
        let tw = TypewriterState::new("Hello world".to_owned(), Duration::from_millis(50));
        assert_eq!(tw.full_text(), "Hello world");
        assert_eq!(tw.visible_text(), "");
    }

    #[test]
    fn multibyte_chars_handled() {
        let mut tw = TypewriterState::new("héllo".to_owned(), Duration::from_millis(50));
        tw.skip();
        assert_eq!(tw.visible_text(), "héllo");
    }
}
