//! Read-only structured history. Keep the server's canonical blocks intact;
//! consumers choose which parts they understand instead of parsing UI text.

use crate::{Result, Value, anyhow, bail};

#[derive(Clone, Debug, PartialEq)]
pub struct ConversationView(Value);

impl ConversationView {
    /// Validate the envelope, retaining all content and unknown descriptive data.
    /// Version negotiation belongs here; executable-history validation stays in Core.
    pub fn from_value(value: Value) -> Result<Self> {
        if value["schema_version"].as_u64() != Some(1) {
            bail!("unsupported ConversationView schema_version");
        }
        if value["projection"].as_str() != Some("conversation") {
            bail!("unsupported ConversationView projection");
        }
        let source = &value["source"];
        for field in ["harness", "version", "session_id"] {
            if source[field].as_str().is_none_or(|text| text.is_empty()) {
                bail!("ConversationView source.{field} is missing");
            }
        }
        let revision = value["conversation"]["revision"]
            .as_u64()
            .ok_or_else(|| anyhow!("ConversationView revision is missing"))?;
        if source["revision"].as_u64() != Some(revision) {
            bail!("ConversationView source revision differs from conversation");
        }
        for field in ["messages", "model_context"] {
            if !value["conversation"][field].is_array() {
                bail!("ConversationView conversation.{field} must be an array");
            }
        }
        Ok(Self(value))
    }

    /// Original complete envelope, including metadata and unfamiliar block kinds.
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    pub fn source(&self) -> &Value {
        &self.0["source"]
    }

    pub fn revision(&self) -> u64 {
        self.0["conversation"]["revision"].as_u64().unwrap()
    }

    /// Current canonical history after compaction, not the raw execution journal.
    pub fn messages(&self) -> &[Value] {
        self.0["conversation"]["messages"].as_array().unwrap()
    }

    /// Request-context observations remain separate from conversation messages.
    pub fn model_context(&self) -> &[Value] {
        self.0["conversation"]["model_context"].as_array().unwrap()
    }

    /// Ordered canonical parts, with their identity, provenance, scope and payload.
    pub fn parts(&self) -> impl Iterator<Item = &Value> {
        self.messages()
            .iter()
            .flat_map(|message| message["parts"].as_array().into_iter().flatten())
    }

    /// Ordinary text only. Reasoning, tool results and media keep their own types.
    pub fn text_parts(&self) -> impl Iterator<Item = &str> {
        self.parts()
            .filter_map(|part| part["payload"]["Text"]["text"].as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    pub(super) fn fixture() -> Value {
        json!({"schema_version":1,"projection":"conversation",
        "source":{"harness":"proteus","version":"test","session_id":"session-1","revision":7},
        "conversation":{"revision":7,"model_context":[{"opaque":"observation"}],"messages":[
            {"id":"message-1","role":"User","metadata":{"org.example/source":true},"parts":[
                {"part_id":"a","payload":{"Text":{"text":"before"}}},
                {"part_id":"b","payload":{"Image":{"image":{"path":"unavailable.png"}}}},
                {"part_id":"c","payload":{"FutureBlock":{"opaque":[1,2,3]}}},
                {"part_id":"d","payload":{"Text":{"text":"after"}}}
            ]}
        ]}})
    }

    #[test]
    fn reading_text_retains_order_media_unknown_data_and_source() {
        let raw = fixture();
        let view = ConversationView::from_value(raw.clone()).unwrap();
        assert_eq!(view.text_parts().collect::<Vec<_>>(), ["before", "after"]);
        assert_eq!(view.parts().count(), 4);
        assert_eq!(view.model_context().len(), 1);
        assert_eq!(view.revision(), 7);
        assert_eq!(view.as_value(), &raw);
    }

    #[test]
    fn unsupported_or_inconsistent_envelopes_fail_explicitly() {
        for (path, bad) in [
            (vec!["schema_version"], json!(2)),
            (vec!["projection"], json!("journal")),
            (vec!["source", "revision"], json!(8)),
            (vec!["conversation", "messages"], Value::Null),
        ] {
            let mut raw = fixture();
            let mut target = &mut raw;
            for field in path {
                target = &mut target[field];
            }
            *target = bad;
            assert!(ConversationView::from_value(raw).is_err());
        }
    }
}
