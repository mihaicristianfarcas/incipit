//! The notes page: one note in a field for writing Markdown, on the reading surface. The note
//! lives in the window for now: the vault will keep it. INCIPIT_PAGE=notes opens it.

use gpui::{div, prelude::*, px, Context, Entity, IntoElement, Render, ScrollHandle, Window};
use incipit_ui::{field, page, TextField};

const SAMPLE: &str = "# Why attention replaces recurrence

A recurrent network reads a sentence **one word at a time**, so the path between two distant words is as long as the gap between them. *Attention* lets every word look at every other in one step.

- The cost: memory grows with the square of the length
- The gain: everything runs in parallel
  - which is why training got so much faster
- [ ] Work through the scaled dot-product by hand

> The claim of the whole paper is in its title.

See [[Scaled dot-product attention]] and the [paper](https://arxiv.org/abs/1706.03762). The score is `softmax(QK^T / sqrt(d_k))`, or $QK^T / \\sqrt{d_k}$ before the softmax.

```
weights = softmax(scores)
```
";

pub struct NotesView {
    note: Entity<TextField>,
    scroll: ScrollHandle,
}

impl NotesView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let note = cx.new(|cx| {
            let mut note = TextField::new(cx).markdown().placeholder("Write");
            note.set_text(SAMPLE, cx);
            note
        });
        NotesView {
            note,
            scroll: ScrollHandle::new(),
        }
    }
}

impl Render for NotesView {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme(window);
        let body = div()
            .pt(px(8.0))
            .child(field(&self.note, t).bare().size(16.0).min_rows(12));
        page("notes", &self.scroll, 680.0, t, body)
    }
}
