//! English Wikipedia's daily Main Page articles, preserved as semantic documents.
mod bounded_html;
mod document;
mod fetch;
mod loader;

pub use document::{
    main_page_titles, parse_article, parse_article_bounded, Block, Document, ImageFloat, ImageRef,
    Span, TableImage, TableSpan,
};
pub use loader::{LoaderEvent, PageLoader};

pub use bounded_html::{ArticleLimits, ARTICLE_ATOM_BASELINE_BYTES, ARTICLE_PARSE_PEAK_BYTES};
