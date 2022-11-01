pub struct Context {
    pub oodle: Option<String>,
}

impl Context {
    pub fn new() -> Self {
        Self { oodle: None }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
