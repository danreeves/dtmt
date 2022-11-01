pub struct Context {}

impl Context {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
