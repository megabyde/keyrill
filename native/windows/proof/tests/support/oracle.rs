use super::{
    client::Client,
    document::{Effect, Span},
};
pub fn document(client: &Client, middle: &str, caret: i32) {
    let expected: Vec<u16> = format!("prefix {middle}suffix").encode_utf16().collect();
    assert_eq!(
        *client.a.text.borrow(),
        expected,
        "whole A document including prefix and suffix"
    );
    assert_eq!(
        client.a.selection.get(),
        Span {
            start: caret,
            length: 0
        }
    );
    assert_eq!(
        *client.b.text.borrow(),
        "prefix suffix".encode_utf16().collect::<Vec<_>>(),
        "B document unchanged"
    );
    assert_eq!(
        client.b.selection.get(),
        Span {
            start: 7,
            length: 0
        }
    );
    client.host.check();
}
pub fn no_effects(client: &Client) {
    assert_eq!(client.a.writes.get(), 0);
    assert_eq!(client.a.starts.get(), 0);
    assert_eq!(client.a.ends.get(), 0);
    assert!(
        !client
            .a
            .operations
            .borrow()
            .iter()
            .any(|op| matches!(op.effect, Effect::Start | Effect::Text | Effect::End))
    );
}

pub fn selection(
    client: &Client,
    middle: &str,
    span: Span,
    style: windows::Win32::UI::TextServices::TF_SELECTIONSTYLE,
) {
    assert_eq!(
        *client.a.text.borrow(),
        format!("prefix {middle}suffix")
            .encode_utf16()
            .collect::<Vec<_>>()
    );
    assert_eq!(client.a.selection.get(), span);
    assert_eq!(client.a.style.get().ase, style.ase);
    assert_eq!(client.a.style.get().fInterimChar, style.fInterimChar);
    assert_eq!(
        *client.b.text.borrow(),
        "prefix suffix".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        client.b.selection.get(),
        Span {
            start: 7,
            length: 0
        }
    );
    client.host.check();
}
