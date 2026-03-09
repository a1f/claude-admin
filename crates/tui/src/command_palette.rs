use crate::input::TextInput;

pub struct CommandPalette {
    pub input: TextInput,
    pub visible: bool,
    pub message: Option<String>,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            input: TextInput::new(":"),
            visible: false,
            message: None,
        }
    }

    pub fn open(&mut self) {
        self.visible = true;
        self.input.clear();
        self.message = None;
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.input.clear();
    }

    pub fn submit(&mut self) -> Option<String> {
        let val = self.input.value().trim().to_string();
        if val.is_empty() {
            return None;
        }
        self.visible = false;
        self.input.clear();
        Some(val)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_palette_not_visible() {
        let palette = CommandPalette::new();
        assert!(!palette.visible);
        assert!(palette.message.is_none());
        assert_eq!(palette.input.value(), "");
    }

    #[test]
    fn test_open_sets_visible() {
        let mut palette = CommandPalette::new();
        palette.open();
        assert!(palette.visible);
        assert!(palette.message.is_none());
    }

    #[test]
    fn test_close_clears() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.input.insert_char('x');
        palette.close();
        assert!(!palette.visible);
        assert_eq!(palette.input.value(), "");
    }

    #[test]
    fn test_typing_accumulates() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.input.insert_char('h');
        palette.input.insert_char('e');
        palette.input.insert_char('l');
        palette.input.insert_char('p');
        assert_eq!(palette.input.value(), "help");
    }

    #[test]
    fn test_submit_returns_value() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.input.insert_char('q');
        palette.input.insert_char('u');
        palette.input.insert_char('i');
        palette.input.insert_char('t');
        let result = palette.submit();
        assert_eq!(result, Some("quit".to_string()));
    }

    #[test]
    fn test_submit_empty_returns_none() {
        let mut palette = CommandPalette::new();
        palette.open();
        let result = palette.submit();
        assert!(result.is_none());
    }

    #[test]
    fn test_submit_clears_and_hides() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.input.insert_char('a');
        let _ = palette.submit();
        assert!(!palette.visible);
        assert_eq!(palette.input.value(), "");
    }
}
