pub fn apply_pattern_string(pattern: &str, vars: &[(&str, &str)]) -> String {
    let mut result = pattern.to_string();
    for (key, value) in vars {
        let needle = format!(":{}", key);
        result = result.replace(&needle, value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_pattern_string() {
        let vars = [("id", "1234"), ("tag", "5678"), ("username", "testuser")];
        assert_eq!(
            apply_pattern_string(":username#:tag", &vars),
            "testuser#5678"
        );

        let nick_vars = [
            ("id", "1234"),
            ("nick", "Test Nick"),
            ("tag", "5678"),
            ("username", "testuser"),
        ];
        assert_eq!(
            apply_pattern_string("[Slack] :nick", &nick_vars),
            "[Slack] Test Nick"
        );
    }
}
