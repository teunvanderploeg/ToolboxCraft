use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Keeps only the currently displayed release, preparing it again when its body changes.
#[derive(Default)]
pub struct ReleaseMarkdown {
    source: String,
    prepared: Option<String>,
    cache: CommonMarkCache,
}

impl ReleaseMarkdown {
    pub fn show(&mut self, ui: &mut egui::Ui, source: &str) {
        if self.source != source {
            self.prepared = prepare(source);
            self.source = source.to_owned();
        }
        ui.scope(|ui| {
            ui.style_mut().url_in_tooltip = true;
            // Let egui apply emphasis and hyperlink colors instead of baking the
            // app's body-text override into every span of the Markdown layout.
            ui.visuals_mut().override_text_color = None;
            if let Some(prepared) = &self.prepared {
                CommonMarkViewer::new().show(ui, &mut self.cache, prepared);
            } else {
                ui.label(source);
            }
        });
    }
}

fn web_url(destination: &str) -> bool {
    // Reject controls rather than letting a URL parser silently strip them.
    !destination.chars().any(char::is_control)
        && url::Url::parse(destination)
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

fn parser_options() -> Options {
    // Match egui_commonmark 0.22's supported extensions.
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
}

/// Sanitize parsed Markdown, so reference links, entities, and escaped destinations
/// receive the same checks as inline links. Keep image alt text without loading any
/// remote, local, or embedded image. HTML stays inert text in the native UI.
fn prepare(source: &str) -> Option<String> {
    let mut links = Vec::new();
    let events = Parser::new_ext(source, parser_options()).filter_map(|event| match event {
        Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => None,
        Event::Start(Tag::Link { ref dest_url, .. }) => {
            let allowed = web_url(dest_url);
            links.push(allowed);
            allowed.then_some(event)
        }
        Event::End(TagEnd::Link) => links.pop().unwrap_or(false).then_some(event),
        Event::Start(Tag::HtmlBlock) | Event::End(TagEnd::HtmlBlock) => None,
        Event::Html(text) | Event::InlineHtml(text) => Some(Event::Code(text)),
        _ => Some(event),
    });
    let mut prepared = String::new();
    // Writing to a String cannot fail.
    pulldown_cmark_to_cmark::cmark(events, &mut prepared).expect("writing Markdown to a String");
    // Validate what the viewer will actually parse: serialization can change the
    // meaning of adjacent text after tags are removed. Fall back to a plain label
    // if the round trip introduces any active content that violates our policy.
    let safe = Parser::new_ext(&prepared, parser_options()).all(|event| match event {
        Event::Start(Tag::Link { dest_url, .. }) => web_url(&dest_url),
        Event::Start(Tag::Image { .. }) | Event::Html(_) | Event::InlineHtml(_) => false,
        _ => true,
    });
    safe.then_some(prepared)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn destinations(source: &str) -> Vec<String> {
        Parser::new_ext(source, parser_options())
            .filter_map(|event| match event {
                Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn preserves_markdown_and_web_links() {
        let prepared = prepare(
            "## Changes\n\n- **Better** *editing* with `PSD` and ~~old~~ fixes\n- [Changelog](https://github.com/storytold/photocraft/releases)\n\n1. First\n2. Second\n\n> A note\n\n```rust\nlet value = 1;\n```",
        ).expect("ordinary Markdown remains formatted");
        let events: Vec<_> = Parser::new_ext(&prepared, parser_options()).collect();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Start(Tag::Heading { .. })))
        );
        for tag in [
            Tag::Strong,
            Tag::Emphasis,
            Tag::Strikethrough,
            Tag::List(None),
            Tag::List(Some(1)),
        ] {
            assert!(events.contains(&Event::Start(tag)));
        }
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Code(text) if text.as_ref() == "PSD"))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Start(Tag::CodeBlock(_))))
        );
        assert_eq!(
            destinations(&prepared),
            ["https://github.com/storytold/photocraft/releases"]
        );
    }

    #[test]
    fn blocks_unsafe_inline_reference_and_autolinks() {
        let source = "[script](javascript:alert%281%29) [file](file:///etc/passwd) [data](data:text/html,test) [app](vscode://file/test) [relative](../secret) [network](//evil.example/test) [encoded](jav&#x61;script:alert%281%29) [ref][bad] <javascript:alert%281%29> [ok](https://example.com)\n\n[bad]: file:///secret";
        let prepared = prepare(source).expect("link labels remain readable");
        assert_eq!(destinations(&prepared), ["https://example.com"]);
        assert!(prepared.contains("script"));
        assert!(prepared.contains("encoded"));
        assert!(prepared.contains("relative"));
        for url in [
            "javascript:alert(1)",
            "file:///tmp/file",
            "data:image/png;base64,AA",
            "https:\n//example.com",
        ] {
            assert!(!web_url(url), "{url}");
        }
        assert!(web_url("HTTPS://example.com/path"));
        assert!(web_url("http://example.com/path"));
    }

    #[test]
    fn images_and_html_remain_inert_even_when_their_text_looks_like_markdown() {
        let prepared = prepare(
            "![Remote image](https://example.com/tracker.png) ![Local image](file:///etc/passwd) ![Embedded image](data:image/png;base64,AA) ![Reference image][image]\n\n[image]: https://example.com/image.png\n\n<script>alert(1)</script>\n\n<img src=\"file:///secret\" onerror=\"alert(1)\">\n\n[\\[click\\]\\(javascript:alert%281%29\\)](javascript:bad)",
        ).expect("inert markup remains readable");
        for event in Parser::new_ext(&prepared, parser_options()) {
            assert!(
                !matches!(
                    event,
                    Event::Start(Tag::Image { .. }) | Event::Html(_) | Event::InlineHtml(_)
                ),
                "{event:?}"
            );
        }
        assert!(destinations(&prepared).is_empty());
        assert!(prepared.contains("Remote image"));
        assert!(prepared.contains("Local image"));
        assert!(prepared.contains("Reference image"));
        assert!(prepared.contains("alert(1)"));
    }
}
