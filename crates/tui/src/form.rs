use crate::input::TextInput;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum FormKind {
    CreateWorkspace,
    CreateProject { workspace_id: i64 },
    CreatePlan { project_id: i64 },
}

pub struct FormField {
    pub input: TextInput,
    pub required: bool,
}

pub struct FormOverlay {
    pub kind: FormKind,
    pub title: String,
    pub fields: Vec<FormField>,
    pub focused_field: usize,
    pub error_message: Option<String>,
}

impl FormOverlay {
    pub fn new_workspace() -> Self {
        Self {
            kind: FormKind::CreateWorkspace,
            title: "Create Workspace".to_string(),
            fields: vec![
                FormField {
                    input: TextInput::new("Path"),
                    required: true,
                },
                FormField {
                    input: TextInput::new("Name (optional)"),
                    required: false,
                },
            ],
            focused_field: 0,
            error_message: None,
        }
    }

    pub fn new_project(workspace_id: i64) -> Self {
        Self {
            kind: FormKind::CreateProject { workspace_id },
            title: "Create Project".to_string(),
            fields: vec![
                FormField {
                    input: TextInput::new("Name"),
                    required: true,
                },
                FormField {
                    input: TextInput::new("Description (optional)"),
                    required: false,
                },
            ],
            focused_field: 0,
            error_message: None,
        }
    }

    pub fn new_plan(project_id: i64) -> Self {
        Self {
            kind: FormKind::CreatePlan { project_id },
            title: "Create Plan".to_string(),
            fields: vec![FormField {
                input: TextInput::new("Name"),
                required: true,
            }],
            focused_field: 0,
            error_message: None,
        }
    }

    pub fn focus_next(&mut self) {
        if !self.fields.is_empty() {
            self.focused_field = (self.focused_field + 1) % self.fields.len();
        }
    }

    pub fn focus_prev(&mut self) {
        if !self.fields.is_empty() {
            if self.focused_field == 0 {
                self.focused_field = self.fields.len() - 1;
            } else {
                self.focused_field -= 1;
            }
        }
    }

    pub fn focused_input(&mut self) -> Option<&mut TextInput> {
        self.fields
            .get_mut(self.focused_field)
            .map(|f| &mut f.input)
    }

    /// Validate required fields. Returns Ok(()) if all required fields are non-empty.
    pub fn validate(&self) -> Result<(), String> {
        for field in &self.fields {
            if field.required && field.input.value().trim().is_empty() {
                return Err(format!("{} is required", field.input.label()));
            }
        }
        Ok(())
    }

    /// Extract field values as a Vec of trimmed strings.
    pub fn field_values(&self) -> Vec<String> {
        self.fields
            .iter()
            .map(|f| f.input.value().trim().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_workspace_has_correct_fields() {
        let form = FormOverlay::new_workspace();
        assert_eq!(form.kind, FormKind::CreateWorkspace);
        assert_eq!(form.fields.len(), 2);
        assert_eq!(form.fields[0].input.label(), "Path");
        assert!(form.fields[0].required);
        assert_eq!(form.fields[1].input.label(), "Name (optional)");
        assert!(!form.fields[1].required);
    }

    #[test]
    fn test_new_project_has_correct_fields() {
        let form = FormOverlay::new_project(42);
        assert_eq!(form.kind, FormKind::CreateProject { workspace_id: 42 });
        assert_eq!(form.fields.len(), 2);
        assert_eq!(form.fields[0].input.label(), "Name");
        assert!(form.fields[0].required);
        assert_eq!(form.fields[1].input.label(), "Description (optional)");
        assert!(!form.fields[1].required);
    }

    #[test]
    fn test_new_plan_has_correct_fields() {
        let form = FormOverlay::new_plan(7);
        assert_eq!(form.kind, FormKind::CreatePlan { project_id: 7 });
        assert_eq!(form.fields.len(), 1);
        assert_eq!(form.fields[0].input.label(), "Name");
        assert!(form.fields[0].required);
    }

    #[test]
    fn test_focus_next_cycles() {
        let mut form = FormOverlay::new_workspace();
        assert_eq!(form.focused_field, 0);
        form.focus_next();
        assert_eq!(form.focused_field, 1);
        form.focus_next();
        assert_eq!(form.focused_field, 0);
    }

    #[test]
    fn test_focus_prev_cycles() {
        let mut form = FormOverlay::new_workspace();
        assert_eq!(form.focused_field, 0);
        form.focus_prev();
        assert_eq!(form.focused_field, 1);
        form.focus_prev();
        assert_eq!(form.focused_field, 0);
    }

    #[test]
    fn test_validate_empty_required_fails() {
        let form = FormOverlay::new_workspace();
        let result = form.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Path is required");
    }

    #[test]
    fn test_validate_filled_required_passes() {
        let mut form = FormOverlay::new_workspace();
        form.fields[0].input.set_value("/home/user/project");
        let result = form.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_optional_can_be_empty() {
        let mut form = FormOverlay::new_workspace();
        form.fields[0].input.set_value("/some/path");
        // Leave field[1] (optional) empty
        assert!(form.validate().is_ok());
    }

    #[test]
    fn test_field_values_returns_trimmed() {
        let mut form = FormOverlay::new_workspace();
        form.fields[0].input.set_value("  /some/path  ");
        form.fields[1].input.set_value("  MyName  ");
        let values = form.field_values();
        assert_eq!(values, vec!["/some/path", "MyName"]);
    }

    #[test]
    fn test_field_values_empty_fields() {
        let form = FormOverlay::new_workspace();
        let values = form.field_values();
        assert_eq!(values, vec!["", ""]);
    }

    #[test]
    fn test_focused_input_returns_correct_field() {
        let mut form = FormOverlay::new_workspace();
        form.focused_input().unwrap().insert_char('x');
        assert_eq!(form.fields[0].input.value(), "x");
        assert_eq!(form.fields[1].input.value(), "");

        form.focus_next();
        form.focused_input().unwrap().insert_char('y');
        assert_eq!(form.fields[0].input.value(), "x");
        assert_eq!(form.fields[1].input.value(), "y");
    }
}
