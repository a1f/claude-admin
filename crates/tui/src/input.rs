/// A single-line text input widget with cursor management.
#[allow(dead_code)]
pub struct TextInput {
    value: String,
    cursor_pos: usize,
    label: String,
}

#[allow(dead_code)]
impl TextInput {
    pub fn new(label: &str) -> Self {
        Self {
            value: String::new(),
            cursor_pos: 0,
            label: label.to_string(),
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.value.insert(self.cursor_pos, c);
        self.cursor_pos += 1;
    }

    pub fn delete_char(&mut self) {
        if self.cursor_pos < self.value.len() {
            self.value.remove(self.cursor_pos);
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.value.remove(self.cursor_pos);
        }
    }

    pub fn move_left(&mut self) {
        self.cursor_pos = self.cursor_pos.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        if self.cursor_pos < self.value.len() {
            self.cursor_pos += 1;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor_pos = self.value.len();
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor_pos = 0;
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn cursor_pos(&self) -> usize {
        self.cursor_pos
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn set_value(&mut self, s: &str) {
        self.value = s.to_string();
        self.cursor_pos = self.value.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_char_basic() {
        let mut input = TextInput::new("test");
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');
        assert_eq!(input.value(), "abc");
        assert_eq!(input.cursor_pos(), 3);
    }

    #[test]
    fn test_insert_char_at_middle() {
        let mut input = TextInput::new("test");
        input.insert_char('a');
        input.insert_char('c');
        input.move_left();
        input.insert_char('b');
        assert_eq!(input.value(), "abc");
        assert_eq!(input.cursor_pos(), 2);
    }

    #[test]
    fn test_delete_char_at_end_is_noop() {
        let mut input = TextInput::new("test");
        input.insert_char('a');
        input.insert_char('b');
        input.delete_char();
        assert_eq!(input.value(), "ab");
        assert_eq!(input.cursor_pos(), 2);
    }

    #[test]
    fn test_delete_char_at_beginning() {
        let mut input = TextInput::new("test");
        input.insert_char('a');
        input.insert_char('b');
        input.move_home();
        input.delete_char();
        assert_eq!(input.value(), "b");
        assert_eq!(input.cursor_pos(), 0);
    }

    #[test]
    fn test_backspace_at_beginning_is_noop() {
        let mut input = TextInput::new("test");
        input.backspace();
        assert_eq!(input.value(), "");
        assert_eq!(input.cursor_pos(), 0);
    }

    #[test]
    fn test_backspace_in_middle() {
        let mut input = TextInput::new("test");
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');
        input.move_left();
        input.backspace();
        assert_eq!(input.value(), "ac");
        assert_eq!(input.cursor_pos(), 1);
    }

    #[test]
    fn test_move_left_clamps_at_zero() {
        let mut input = TextInput::new("test");
        input.move_left();
        assert_eq!(input.cursor_pos(), 0);
        input.move_left();
        assert_eq!(input.cursor_pos(), 0);
    }

    #[test]
    fn test_move_right_clamps_at_len() {
        let mut input = TextInput::new("test");
        input.insert_char('a');
        input.insert_char('b');
        assert_eq!(input.cursor_pos(), 2);
        input.move_right();
        assert_eq!(input.cursor_pos(), 2);
    }

    #[test]
    fn test_move_home_and_move_end() {
        let mut input = TextInput::new("test");
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');
        assert_eq!(input.cursor_pos(), 3);

        input.move_home();
        assert_eq!(input.cursor_pos(), 0);

        input.move_end();
        assert_eq!(input.cursor_pos(), 3);
    }

    #[test]
    fn test_clear_resets_everything() {
        let mut input = TextInput::new("test");
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');
        input.clear();
        assert_eq!(input.value(), "");
        assert_eq!(input.cursor_pos(), 0);
    }

    #[test]
    fn test_set_value_sets_cursor_to_end() {
        let mut input = TextInput::new("test");
        input.set_value("hello");
        assert_eq!(input.value(), "hello");
        assert_eq!(input.cursor_pos(), 5);
    }

    #[test]
    fn test_label_returns_label() {
        let input = TextInput::new("Command");
        assert_eq!(input.label(), "Command");
    }
}
