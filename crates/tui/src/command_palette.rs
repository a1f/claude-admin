use crate::input::TextInput;

const COMMANDS: &[&str] = &[
    "ws add", "ws list", "ws del", "proj new", "proj del", "plan del", "help", "quit",
];

pub struct CommandPalette {
    pub input: TextInput,
    pub visible: bool,
    pub message: Option<String>,
    pub suggestions: Vec<String>,
    pub selected_suggestion: usize,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            input: TextInput::new(":"),
            visible: false,
            message: None,
            suggestions: Vec::new(),
            selected_suggestion: 0,
        }
    }

    pub fn open(&mut self) {
        self.visible = true;
        self.input.clear();
        self.message = None;
        self.update_suggestions();
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.input.clear();
        self.suggestions.clear();
        self.selected_suggestion = 0;
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

    pub fn update_suggestions(&mut self) {
        let prefix = self.input.value().trim().to_lowercase();
        if prefix.is_empty() {
            self.suggestions = COMMANDS.iter().map(|s| (*s).to_string()).collect();
        } else {
            self.suggestions = COMMANDS
                .iter()
                .filter(|cmd| cmd.starts_with(&prefix))
                .map(|s| (*s).to_string())
                .collect();
        }
        self.selected_suggestion = 0;
    }

    pub fn select_next_suggestion(&mut self) {
        if !self.suggestions.is_empty() {
            self.selected_suggestion = (self.selected_suggestion + 1) % self.suggestions.len();
        }
    }

    pub fn select_prev_suggestion(&mut self) {
        if !self.suggestions.is_empty() {
            if self.selected_suggestion == 0 {
                self.selected_suggestion = self.suggestions.len() - 1;
            } else {
                self.selected_suggestion -= 1;
            }
        }
    }

    pub fn accept_suggestion(&mut self) {
        if let Some(suggestion) = self.suggestions.get(self.selected_suggestion).cloned() {
            self.input.set_value(&suggestion);
            self.input.insert_char(' ');
            self.suggestions.clear();
        }
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

    #[test]
    fn test_prefix_matching_ws() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.input.insert_char('w');
        palette.update_suggestions();
        assert_eq!(palette.suggestions.len(), 3);
        assert!(palette.suggestions.iter().all(|s| s.starts_with("ws")));
    }

    #[test]
    fn test_prefix_matching_no_match() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.input.set_value("xyz");
        palette.update_suggestions();
        assert!(palette.suggestions.is_empty());
    }

    #[test]
    fn test_empty_input_shows_all() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.update_suggestions();
        assert_eq!(palette.suggestions.len(), COMMANDS.len());
    }

    #[test]
    fn test_tab_accepts_suggestion() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.input.insert_char('h');
        palette.update_suggestions();
        assert_eq!(palette.suggestions.len(), 1);
        palette.accept_suggestion();
        assert_eq!(palette.input.value(), "help ");
    }

    #[test]
    fn test_up_down_cycle_suggestions() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.input.insert_char('w');
        palette.update_suggestions();
        assert_eq!(palette.selected_suggestion, 0);
        palette.select_next_suggestion();
        assert_eq!(palette.selected_suggestion, 1);
        palette.select_next_suggestion();
        assert_eq!(palette.selected_suggestion, 2);
        palette.select_next_suggestion();
        assert_eq!(palette.selected_suggestion, 0);
    }

    #[test]
    fn test_selection_wraps_backwards() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.input.insert_char('w');
        palette.update_suggestions();
        palette.select_prev_suggestion();
        assert_eq!(palette.selected_suggestion, 2);
    }
}
