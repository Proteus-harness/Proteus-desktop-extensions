//! Headless consumer of the same API available to installed extensions.
//! cargo run -p proteus-extension --example conversation-reader -- ORIGIN SESSION_DIR
//! Supply the server token through PROTEUS_SERVER_TOKEN, never a command-line argument.

use proteus_extension::{Host, Result, Server, anyhow, json};

fn main() -> Result {
    let mut args = std::env::args().skip(1);
    let origin = args
        .next()
        .ok_or_else(|| anyhow!("missing server origin"))?;
    let session = args
        .next()
        .ok_or_else(|| anyhow!("missing session directory"))?;
    if args.next().is_some() {
        return Err(anyhow!("unexpected argument"));
    }
    let mut host = Host::default();
    host.server = Some(Server {
        origin,
        token: std::env::var("PROTEUS_SERVER_TOKEN")?,
    });
    let history = host.conversation_for(Some(&session))?;
    // Inspect structure without printing private conversation text.
    println!(
        "{}",
        json!({
            "source":history.source(), "revision":history.revision(),
            "messages":history.messages().len(), "parts":history.parts().count(),
            "text_characters":history.text_parts().map(|text| text.chars().count()).sum::<usize>(),
            "model_context_observations":history.model_context().len(),
        })
    );
    Ok(())
}
