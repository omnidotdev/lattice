/// Expand `${VAR}` patterns in a string using the provided lookup function.
fn expand_vars_with(input: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut var_name = String::new();
            for c in chars.by_ref() {
                if c == '}' {
                    break;
                }
                var_name.push(c);
            }
            if let Some(val) = lookup(&var_name) {
                result.push_str(&val);
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Expand `${VAR}` patterns in a string using environment variables.
/// Unset variables are replaced with empty string.
pub fn expand_vars(input: &str) -> String {
    expand_vars_with(input, |name| std::env::var(name).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_lookup(name: &str) -> Option<String> {
        match name {
            "TEST_EV_A" => Some("hello".into()),
            _ => None,
        }
    }

    #[test]
    fn expand_single_var() {
        assert_eq!(expand_vars_with("${TEST_EV_A}", test_lookup), "hello");
    }

    #[test]
    fn expand_unset_var_is_empty() {
        assert_eq!(
            expand_vars_with("prefix-${UNSET_VAR_XYZ}-suffix", test_lookup),
            "prefix--suffix"
        );
    }

    #[test]
    fn no_vars_unchanged() {
        assert_eq!(expand_vars_with("no variables here", test_lookup), "no variables here");
    }

    #[test]
    fn dollar_without_brace_unchanged() {
        assert_eq!(expand_vars_with("price is $5", test_lookup), "price is $5");
    }
}
