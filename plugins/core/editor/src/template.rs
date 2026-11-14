//! Single-pass substitution of `{{name}}` placeholders in the plugin's prompt templates.
//!
//! Values go in verbatim: no HTML escaping, and a value that itself contains `{{name}}` is not
//! substituted again. Text from a source file reaches an agent exactly as it was written.

/// Replaces each `{{key}}` of `template` by the value paired with `key`. A placeholder with no
/// value stays as written.
pub fn render(template: &str, values: &[(&str, &str)]) -> String {
    let mut output = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        output.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            output.push_str(&rest[open..]);
            return output;
        };
        let key = &after[..close];
        match values.iter().find(|(name, _)| *name == key) {
            Some((_, value)) => output.push_str(value),
            None => {
                output.push_str("{{");
                output.push_str(key);
                output.push_str("}}");
            }
        }
        rest = &after[close + 2..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ: F-023
    #[test]
    fn values_are_inserted_verbatim_and_never_resubstituted() {
        let rendered = render("a {{x}} b {{y}}", &[("x", "{{y}} <&>"), ("y", "Y")]);
        assert_eq!(rendered, "a {{y}} <&> b Y");
    }

    /// REQ: F-023
    #[test]
    fn unknown_and_unterminated_placeholders_stay() {
        assert_eq!(render("{{z}} {{", &[]), "{{z}} {{");
    }
}
