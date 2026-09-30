// This build-only copy validates the catalog; public runtime constants are not
// referenced individually by the build script.
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
        registry
            .register_template_string(name, source)
            .unwrap_or_else(|error| panic!("invalid embedded prompt {name}: {error}"));
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    let mut files = std::collections::HashSet::new();
    collect_templates(&root, &root, &mut files);
    let embedded: std::collections::HashSet<String> = names
        .into_iter()
        .map(|name| format!("{name}.hbs"))
        .collect();
    assert_eq!(
        files, embedded,
        "every .hbs file must have exactly one catalog entry"
    );
}

fn collect_templates(
    root: &std::path::Path,
    directory: &std::path::Path,
    files: &mut std::collections::HashSet<String>,
) {
    for entry in std::fs::read_dir(directory).expect("template sources available at build time") {
        let path = entry.expect("read template entry").path();
        if path.is_dir() {
            collect_templates(root, &path, files);
        } else if path.extension().is_some_and(|extension| extension == "hbs") {
            files.insert(
                path.strip_prefix(root)
                    .expect("template under root")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}
