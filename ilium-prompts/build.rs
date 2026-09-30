#[allow(dead_code)]
#[path = "src/catalog.rs"]
mod catalog;

fn main() {
    println!("cargo:rerun-if-changed=templates");
    println!("cargo:rerun-if-changed=src");
    let mut registry = handlebars::Handlebars::new();
    registry.register_escape_fn(handlebars::no_escape);
    let mut names = std::collections::HashSet::new();
    for &(name, source) in catalog::catalog() {
        assert!(names.insert(name), "duplicate prompt catalog name: {name}");
        registry.register_template_string(name, source)
            .unwrap_or_else(|error| panic!("invalid embedded prompt {name}: {error}"));
    }
}
