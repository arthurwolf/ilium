//! English Wikipedia's daily Main Page articles, preserved as semantic documents.
mod document;
mod fetch;
mod loader;

pub use document::{
    main_page_titles, parse_article, Block, Document, ImageFloat, ImageRef, Span, TableImage,
    TableSpan,
};
pub use loader::{LoaderEvent, PageLoader};
