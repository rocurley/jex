use rustyline::{
    completion::{Completer, FilenameCompleter},
    highlight::Highlighter,
    hint::Hinter,
    line_buffer::LineBuffer,
    validate::Validator,
    Context, Helper as HelperTrait, Result,
};

pub struct Helper {
    completer: FilenameCompleter,
}
impl Helper {
    pub fn new() -> Self {
        Helper {
            completer: FilenameCompleter::new(),
        }
    }
}
impl Completer for Helper {
    type Candidate = <FilenameCompleter as Completer>::Candidate;
    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Self::Candidate>)> {
        self.completer.complete(line, pos, ctx)
    }
    fn update(&self, line: &mut LineBuffer, start: usize, elected: &str) {
        self.completer.update(line, start, elected)
    }
}
impl Highlighter for Helper {}
impl Hinter for Helper {}
impl Validator for Helper {}
impl HelperTrait for Helper {}
