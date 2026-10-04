//! Bounded scraper DOM construction over the installed html5ever TreeSink.
//! Pure parser: its caller must admit the declared peak before entering.
use ego_tree::{NodeId, Tree};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{Attribute, LocalName, Namespace, QualName};
use scraper::{Html, HtmlTreeSink, Node};
use std::{borrow::Cow, cell::Cell};

pub const ARTICLE_PARSE_PEAK_BYTES: usize = 192 * 1024 * 1024;
pub const ARTICLE_ATOM_BASELINE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct ArticleLimits {
    pub dom_nodes: usize,
    pub dom_attributes: usize,
    pub dom_bytes: usize,
    pub class_tokens: usize,
    pub tag_bytes: usize,
    pub table_cells: usize,
}
impl Default for ArticleLimits {
    fn default() -> Self {
        Self {
            dom_nodes: 32_768,
            dom_attributes: 131_072,
            dom_bytes: 16 * 1024 * 1024,
            class_tokens: 131_072,
            tag_bytes: 65_536,
            table_cells: 100_000,
        }
    }
}
impl ArticleLimits {
    fn validate(self) -> Result<(), String> {
        let cap = Self::default();
        if self.dom_nodes < 4
            || self.dom_nodes > cap.dom_nodes
            || self.dom_attributes == 0
            || self.dom_attributes > cap.dom_attributes
            || self.dom_bytes == 0
            || self.dom_bytes > cap.dom_bytes
            || self.class_tokens == 0
            || self.class_tokens > cap.class_tokens
            || self.tag_bytes == 0
            || self.tag_bytes > cap.tag_bytes
            || self.table_cells == 0
            || self.table_cells > cap.table_cells
        {
            return Err("invalid bounded Wikipedia parser limits".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
struct Handle {
    id: Option<NodeId>,
    serial: usize,
    name: QualName,
}
#[derive(Debug, Clone, Copy)]
struct Usage {
    nodes: usize,
    attributes: usize,
    bytes: usize,
    classes: usize,
}
struct BoundedSink {
    inner: HtmlTreeSink,
    limits: ArticleLimits,
    usage: Cell<Usage>,
    serial: Cell<usize>,
    error: Cell<Option<&'static str>>,
}
impl BoundedSink {
    fn new(limits: ArticleLimits) -> Self {
        let mut html = Html::new_document();
        // Exact bounded arena capacity; no geometric node-Vec growth.
        html.tree = Tree::with_capacity(Node::Document, limits.dom_nodes);
        Self {
            inner: HtmlTreeSink::new(html),
            limits,
            usage: Cell::new(Usage {
                nodes: 1,
                attributes: 0,
                bytes: 0,
                classes: 0,
            }),
            serial: Cell::new(0),
            error: Cell::new(None),
        }
    }
    fn admit(&self, nodes: usize, attrs: usize, bytes: usize, classes: usize) -> bool {
        if self.error.get().is_some() {
            return false;
        }
        let old = self.usage.get();
        let Some(next) = old
            .nodes
            .checked_add(nodes)
            .zip(old.attributes.checked_add(attrs))
            .zip(old.bytes.checked_add(bytes))
            .zip(old.classes.checked_add(classes))
            .map(|(((nodes, attributes), bytes), classes)| Usage {
                nodes,
                attributes,
                bytes,
                classes,
            })
        else {
            self.error.set(Some("Wikipedia DOM accounting overflow"));
            return false;
        };
        if next.nodes > self.limits.dom_nodes
            || next.attributes > self.limits.dom_attributes
            || next.bytes > self.limits.dom_bytes
            || next.classes > self.limits.class_tokens
        {
            self.error.set(Some(
                "Wikipedia DOM exceeds node/attribute/string/class limit",
            ));
            return false;
        }
        self.usage.set(next);
        true
    }
    fn handle(&self, id: Option<NodeId>, name: QualName) -> Handle {
        let serial = self.serial.get().saturating_add(1);
        self.serial.set(serial);
        Handle { id, serial, name }
    }
    fn root_name() -> QualName {
        QualName::new(
            None,
            Namespace::from("http://www.w3.org/1999/xhtml"),
            LocalName::from("html"),
        )
    }
    fn attrs(attrs: &[Attribute]) -> Option<(usize, usize)> {
        let mut bytes = 0usize;
        let mut classes = 0usize;
        for attr in attrs {
            bytes = bytes
                .checked_add(attr.name.local.len())?
                .checked_add(attr.name.ns.len())?
                .checked_add(attr.value.len())?;
            if attr.name.local.as_ref() == "class" {
                classes = classes.checked_add(attr.value.split_ascii_whitespace().count())?;
            }
        }
        Some((bytes, classes))
    }
    fn text_admission(&self, parent: NodeId, text: &StrTendril, before: bool) -> bool {
        let html = self.inner.0.borrow();
        let adjacent = html.tree.get(parent).and_then(|node| {
            if before {
                node.prev_sibling()
            } else {
                node.last_child()
            }
        });
        let is_text = adjacent.is_some_and(|node| node.value().as_text().is_some());
        self.admit(if is_text { 0 } else { 1 }, 0, text.len(), 0)
    }
    fn child(&self, child: NodeOrText<Handle>) -> Option<NodeOrText<NodeId>> {
        match child {
            NodeOrText::AppendNode(handle) => handle.id.map(NodeOrText::AppendNode),
            NodeOrText::AppendText(text) => Some(NodeOrText::AppendText(text)),
        }
    }
}
impl TreeSink for BoundedSink {
    type Handle = Handle;
    type Output = Result<Html, String>;
    type ElemName<'a> = &'a QualName;
    fn finish(self) -> Self::Output {
        if let Some(error) = self.error.get() {
            Err(error.into())
        } else {
            Ok(self.inner.finish())
        }
    }
    fn parse_error(&self, _message: Cow<'static, str>) { /* Parse-error lists do not affect article semantics; do not allocate them. */
    }
    fn get_document(&self) -> Handle {
        Handle {
            id: Some(self.inner.get_document()),
            serial: 0,
            name: Self::root_name(),
        }
    }
    fn elem_name<'a>(&'a self, target: &'a Handle) -> &'a QualName {
        &target.name
    }
    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        match (x.id, y.id) {
            (Some(x), Some(y)) => x == y,
            _ => x.serial == y.serial,
        }
    }
    fn set_quirks_mode(&self, mode: QuirksMode) {
        if self.error.get().is_none() {
            self.inner.set_quirks_mode(mode);
        }
    }
    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        let counts = Self::attrs(&attrs);
        let nodes = if name.ns.as_ref() == "http://www.w3.org/1999/xhtml"
            && name.local.as_ref() == "template"
        {
            2
        } else {
            1
        };
        let id = if let Some((bytes, classes)) = counts {
            if self.admit(
                nodes,
                attrs.len(),
                bytes + name.local.len() + name.ns.len(),
                classes,
            ) {
                Some(self.inner.create_element(name.clone(), attrs, flags))
            } else {
                None
            }
        } else {
            self.error
                .set(Some("Wikipedia DOM attribute size overflow"));
            None
        };
        self.handle(id, name)
    }
    fn create_comment(&self, text: StrTendril) -> Handle {
        let id = if self.admit(1, 0, text.len(), 0) {
            Some(self.inner.create_comment(text))
        } else {
            None
        };
        self.handle(id, Self::root_name())
    }
    fn create_pi(&self, target: StrTendril, data: StrTendril) -> Handle {
        let id = if self.admit(1, 0, target.len() + data.len(), 0) {
            Some(self.inner.create_pi(target, data))
        } else {
            None
        };
        self.handle(id, Self::root_name())
    }
    fn append_doctype_to_document(&self, name: StrTendril, public: StrTendril, system: StrTendril) {
        if self.admit(1, 0, name.len() + public.len() + system.len(), 0) {
            self.inner.append_doctype_to_document(name, public, system);
        }
    }
    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        if self.error.get().is_some() {
            return;
        }
        let Some(parent) = parent.id else {
            return;
        };
        if let NodeOrText::AppendText(ref text) = child {
            if !self.text_admission(parent, text, false) {
                return;
            }
        }
        if let Some(child) = self.child(child) {
            self.inner.append(&parent, child);
        }
    }
    fn append_before_sibling(&self, sibling: &Handle, child: NodeOrText<Handle>) {
        if self.error.get().is_some() {
            return;
        }
        let Some(sibling) = sibling.id else {
            return;
        };
        if let NodeOrText::AppendText(ref text) = child {
            if !self.text_admission(sibling, text, true) {
                return;
            }
        }
        if let Some(child) = self.child(child) {
            self.inner.append_before_sibling(&sibling, child);
        }
    }
    fn append_based_on_parent_node(
        &self,
        element: &Handle,
        previous: &Handle,
        child: NodeOrText<Handle>,
    ) {
        if self.error.get().is_some() {
            return;
        }
        let has_parent = element
            .id
            .and_then(|id| {
                self.inner
                    .0
                    .borrow()
                    .tree
                    .get(id)
                    .map(|node| node.parent().is_some())
            })
            .unwrap_or(false);
        if has_parent {
            self.append_before_sibling(element, child)
        } else {
            self.append(previous, child)
        }
    }
    fn remove_from_parent(&self, target: &Handle) {
        if self.error.get().is_some() {
            return;
        }
        let Some(id) = target.id else {
            return;
        };
        self.inner.remove_from_parent(&id);
    }
    fn reparent_children(&self, node: &Handle, parent: &Handle) {
        if self.error.get().is_some() {
            return;
        }
        let (Some(node), Some(parent)) = (node.id, parent.id) else {
            return;
        };
        self.inner.reparent_children(&node, &parent);
    }
    fn add_attrs_if_missing(&self, target: &Handle, attrs: Vec<Attribute>) {
        if self.error.get().is_some() {
            return;
        }
        let Some(id) = target.id else {
            return;
        };
        if let Some((bytes, classes)) = Self::attrs(&attrs) {
            if self.admit(0, attrs.len(), bytes, classes) {
                self.inner.add_attrs_if_missing(&id, attrs);
            }
        } else {
            self.error
                .set(Some("Wikipedia DOM attribute size overflow"));
        }
    }
    fn get_template_contents(&self, target: &Handle) -> Handle {
        let id = if self.error.get().is_none() {
            target.id.map(|id| self.inner.get_template_contents(&id))
        } else {
            None
        };
        self.handle(id, Self::root_name())
    }
}

// A tag must be bounded BEFORE html5ever builds its attribute Vec or interns
// all names. Match HTML's raw attribute quote states; quotes inside unquoted
// values are literal, preventing a malformed quote from hiding a long token.
fn preflight_tags(html: &str, limit: usize) -> Result<(), String> {
    #[derive(Clone, Copy)]
    enum State {
        Name,
        Before,
        Attr,
        After,
        Value,
        Double,
        Single,
        Unquoted,
        QuotedEnd,
        Slash,
    }
    let bytes = html.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        if bytes.get(index) == Some(&b'/') {
            index += 1;
        }
        if !bytes.get(index).is_some_and(u8::is_ascii_alphabetic) {
            continue;
        }
        let mut state = State::Name;
        while let Some(&byte) = bytes.get(index) {
            if index - start >= limit {
                return Err("Wikipedia HTML tag exceeds tokenizer allocation limit".into());
            }
            let space = matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 12);
            use State::*;
            state = match state {
                Double => {
                    if byte == b'"' {
                        QuotedEnd
                    } else {
                        Double
                    }
                }
                Single => {
                    if byte == b'\'' {
                        QuotedEnd
                    } else {
                        Single
                    }
                }
                Name => {
                    if space {
                        Before
                    } else if byte == b'/' {
                        Slash
                    } else if byte == b'>' {
                        break;
                    } else {
                        Name
                    }
                }
                Before | Slash | QuotedEnd => {
                    if space {
                        Before
                    } else if byte == b'/' {
                        Slash
                    } else if byte == b'>' {
                        break;
                    } else {
                        Attr
                    }
                }
                Attr | After => {
                    if space {
                        After
                    } else if byte == b'=' {
                        Value
                    } else if byte == b'/' {
                        Slash
                    } else if byte == b'>' {
                        break;
                    } else {
                        Attr
                    }
                }
                Value => {
                    if space {
                        Value
                    } else if byte == b'"' {
                        Double
                    } else if byte == b'\'' {
                        Single
                    } else if byte == b'>' {
                        break;
                    } else {
                        Unquoted
                    }
                }
                Unquoted => {
                    if space {
                        Before
                    } else if byte == b'>' {
                        break;
                    } else {
                        Unquoted
                    }
                }
            };
            index += 1;
        }
        index += 1;
    }
    Ok(())
}
pub(crate) fn parse(html: &str, limits: ArticleLimits) -> Result<Html, String> {
    limits.validate()?;
    if html.len() > super::document::MAX_HTML_BYTES {
        return Err("Wikipedia HTML exceeds 16 MiB; article was not truncated".into());
    }
    preflight_tags(html, limits.tag_bytes)?;
    let sink = BoundedSink::new(limits);
    let mut parser = html5ever::driver::parse_document(sink, Default::default());
    let mut offset = 0;
    while offset < html.len() {
        let mut end = (offset + 256).min(html.len());
        while !html.is_char_boundary(end) {
            end -= 1;
        }
        parser.process(StrTendril::from_slice(&html[offset..end]));
        if let Some(error) = parser.tokenizer.sink.sink.error.get() {
            return Err(error.into());
        }
        offset = end;
    }
    parser.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_sink_refuses_before_arena_growth_and_finishes_with_error() {
        let limits = ArticleLimits {
            dom_nodes: 4,
            ..ArticleLimits::default()
        };
        let sink = BoundedSink::new(limits);
        for _ in 0..20 {
            sink.create_element(
                BoundedSink::root_name(),
                Vec::new(),
                ElementFlags::default(),
            );
        }
        assert_eq!(sink.inner.0.borrow().tree.nodes().count(), 4);
        assert_eq!(sink.usage.get().nodes, 4);
        assert!(sink.finish().is_err());
    }
    #[test]
    fn adjacent_text_coalescing_is_counted_as_one_actual_node() {
        let sink = BoundedSink::new(ArticleLimits::default());
        let root = sink.get_document();
        for _ in 0..100 {
            sink.append(&root, NodeOrText::AppendText(StrTendril::from_slice("x")));
        }
        assert_eq!(sink.inner.0.borrow().tree.nodes().count(), 2);
        assert_eq!(sink.usage.get().nodes, 2);
        assert_eq!(sink.usage.get().bytes, 100);
    }
    #[test]
    fn malformed_attribute_quotes_cannot_hide_an_oversized_real_tag() {
        let html = format!("<p ==\"prefix>{}\" a='b'>x</p>", "a".repeat(1000));
        assert!(preflight_tags(&html, 64).is_err());
    }
    #[test]
    fn class_tokens_are_limited_before_scraper_lazy_class_vector_allocation() {
        let limits = ArticleLimits {
            class_tokens: 2,
            ..ArticleLimits::default()
        };
        assert!(parse("<body><p class='a b c'>x</p></body>", limits).is_err());
    }
}
