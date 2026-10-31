use proteus_extension::ui::{self, Node, fill};
use proteus_extension::{Extension, Host, Result, Surface, json, run};

#[derive(Default)]
struct Terminal;

impl Extension for Terminal {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface == Surface::Preview {
            host.show(view, preview());
            return Ok(());
        }
        let root = fill(vec![
            json!({"type": "terminal", "id": "shell", "cwd": host.workspace}),
        ]);
        host.show(view, root);
        Ok(())
    }
}

fn preview() -> proteus_extension::Value {
    // The read-only code primitive shows synthetic output without creating a
    // terminal node, PTY, shell or process in the user's workspace.
    ui::stack(vec![
        ui::muted("Пример вывода"),
        ui::code(
            "terminal-output-example",
            "Сборка проекта…\nПроверка: 24 теста пройдено\nГотово за 1,2 с\n",
            "text",
        )
        .with("embedded", true),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_demo_never_creates_a_live_terminal() {
        let root = preview();
        assert_eq!(root["children"][1]["type"], "code");
        assert_eq!(root["children"][1]["embedded"], true);
        assert!(!root.to_string().contains("\"type\":\"terminal\""));
        assert!(!root.to_string().contains("cwd"));
    }
}

fn main() {
    run(Terminal);
}
