use proteus_extension::ui::fill;
use proteus_extension::{Extension, Host, Result, Surface, json, run};

#[derive(Default)]
struct Terminal;

impl Extension for Terminal {
    fn render(&mut self, host: &mut Host, view: &str, _: Surface, _: Option<&str>) -> Result {
        let root = fill(vec![
            json!({"type": "terminal", "id": "shell", "cwd": host.workspace}),
        ]);
        host.show(view, root);
        Ok(())
    }
}

fn main() {
    run(Terminal);
}
