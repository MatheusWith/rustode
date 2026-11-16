//! The commit and branch conventions from `CONTRIBUTING.md`, as code.

/// Allowed commit types; also the allowed branch prefixes.
pub const TYPES: &[&str] = &[
    "feat", "fix", "perf", "refactor", "test", "docs", "build", "ci", "chore", "revert",
];

/// Allowed commit scopes: one per crate, plus cross-cutting areas.
pub const SCOPES: &[&str] = &[
    "core",
    "syntax",
    "fs",
    "config",
    "workspace",
    "protocol",
    "trace",
    "cli",
    "tui",
    "gui",
    "xtask",
    "bench",
    "deps",
    "docs",
    "ci",
    "hooks",
];

/// Longest allowed title line.
pub const MAX_TITLE: usize = 72;

/// Where every rejection points the author.
pub const GUIDE: &str = "see CONTRIBUTING.md, \"Editor code: commits, branches and checks\"";

/// Checks a commit message; returns one line per problem (empty means valid).
/// Lines starting with `#` are git comments and are ignored.
pub fn check_message(message: &str) -> Vec<String> {
    let lines: Vec<&str> = message.lines().filter(|l| !l.starts_with('#')).collect();
    let Some(title) = lines.iter().find(|l| !l.trim().is_empty()) else {
        return vec!["the message is empty".to_owned()];
    };
    let mut problems = check_title(title);
    problems.extend(lines.iter().filter_map(|l| check_trailer(l)));
    problems
}

fn check_title(title: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if title.chars().count() > MAX_TITLE {
        problems.push(format!("the title is longer than {MAX_TITLE} characters"));
    }
    let Some((head, subject)) = title.split_once(": ") else {
        problems.push(format!("the title '{title}' is not 'type(scope): subject'"));
        return problems;
    };
    let head = head.strip_suffix('!').unwrap_or(head);
    let (kind, scope) = match head.split_once('(') {
        Some((kind, rest)) => (kind, rest.strip_suffix(')')),
        None => (head, None),
    };
    if !TYPES.contains(&kind) {
        problems.push(format!(
            "unknown type '{kind}'; allowed: {}",
            TYPES.join(", ")
        ));
    }
    if head.contains('(') && !scope.is_some_and(|s| SCOPES.contains(&s)) {
        problems.push(format!(
            "unknown scope in '{head}'; allowed: {}",
            SCOPES.join(", ")
        ));
    }
    problems.extend(check_subject(subject));
    problems
}

fn check_subject(subject: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if subject.trim().is_empty() {
        problems.push("the subject is empty".to_owned());
    }
    if subject.ends_with('.') {
        problems.push("the subject ends with a period".to_owned());
    }
    if subject.chars().next().is_some_and(char::is_uppercase) {
        problems.push("the subject must start in lower case".to_owned());
    }
    problems
}

fn check_trailer(line: &str) -> Option<String> {
    let value = line.strip_prefix("Co-Authored-By:")?.trim();
    let well_formed = value
        .strip_suffix('>')
        .and_then(|v| v.rsplit_once(" <"))
        .is_some_and(|(name, email)| {
            !name.trim().is_empty() && email.contains('@') && !email.contains(' ')
        });
    (!well_formed).then(|| format!("the trailer '{line}' is not 'Co-Authored-By: Name <email>'"))
}

/// Checks a branch name such as `feat/add-rope-buffer`.
pub fn check_branch(name: &str) -> Option<String> {
    let valid = name.split_once('/').is_some_and(|(kind, rest)| {
        TYPES.contains(&kind)
            && !rest.is_empty()
            && rest.split('-').all(|w| {
                !w.is_empty()
                    && w.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            })
    });
    (!valid).then(|| format!("the branch name '{name}' is not '<type>/<lower-case-words>'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaking_change_marker_and_no_scope_are_valid() {
        assert_eq!(
            check_message("feat!: drop the old api\n"),
            Vec::<String>::new()
        );
        assert_eq!(check_message("chore: tidy\n"), Vec::<String>::new());
    }

    #[test]
    fn empty_message_is_rejected() {
        assert_eq!(
            check_message("# only a comment\n\n"),
            ["the message is empty"]
        );
    }

    #[test]
    fn branch_names() {
        assert_eq!(check_branch("feat/rope-buffer-2"), None);
        assert!(check_branch("feat/Rope").is_some());
        assert!(check_branch("feat/").is_some());
        assert!(check_branch("feature/x").is_some());
        assert!(check_branch("main").is_some());
    }
}
