//! One fixture per grammar: a few lines of the language, and the kinds a
//! few of their tokens must get. The kinds stand for the `TextMate` scopes
//! of VS Code's grammars, resolved through Dark Modern (`grammar.rs` docs).

use rustode_syntax::{Syntax, TokenKind as K};

/// The highlighted tokens of `text` as language `id`, with their text.
fn tokens<'a>(id: &str, text: &'a str) -> Vec<(&'a str, K)> {
    let syntax = Syntax::new(id, text).unwrap_or_else(|| panic!("no grammar for {id}"));
    syntax
        .highlights(text, 0..text.len())
        .into_iter()
        .map(|span| (&text[span.start..span.end], span.kind))
        .collect()
}

/// Asserts each `(token, kind)` is among the highlighted tokens.
fn expect(id: &str, text: &str, wanted: &[(&str, K)]) {
    let found = tokens(id, text);
    for want in wanted {
        assert!(
            found.contains(want),
            "{id}: {want:?} not in {found:#?}\nfor {text:?}"
        );
    }
}

#[test]
fn json_and_jsonc() {
    let text = "{\"a\": [1, true, null, \"x\\n\"]} // c\n";
    let wanted = [
        ("\"a\"", K::Variable),
        ("1", K::Number),
        ("true", K::Boolean),
        ("null", K::Boolean),
        ("\\n", K::Escape),
        ("// c", K::Comment),
    ];
    expect("json", text, &wanted);
    expect("jsonc", text, &wanted);
}

#[test]
fn toml() {
    expect(
        "toml",
        "[package]\nname = \"x\" # c\nn = 1\nb = true\n",
        &[
            ("name", K::Variable),
            ("=", K::Operator),
            ("\"x\"", K::String),
            ("# c", K::Comment),
            ("1", K::Number),
            ("true", K::Boolean),
            ("[", K::Punctuation),
        ],
    );
}

#[test]
fn markdown_blocks() {
    expect(
        "markdown",
        "# Title\n\n- item\n\n```rust\nfn x(){}\n```\n",
        &[
            ("#", K::Punctuation),
            ("Title", K::Keyword),
            ("- ", K::Punctuation),
            ("```", K::Punctuation),
        ],
    );
}

#[test]
fn javascript_and_jsx() {
    let text =
        "// c\nfunction f(a) { if (a) return this.x; }\nlet o = new Foo(`t${1}`, null);\nf();\n";
    for id in ["javascript", "javascriptreact"] {
        expect(
            id,
            text,
            &[
                ("// c", K::Comment),
                ("function", K::Keyword),
                ("f", K::Function),
                ("if", K::ControlKeyword),
                ("return", K::ControlKeyword),
                ("this", K::Keyword),
                ("Foo", K::Type),
                ("null", K::Boolean),
                ("1", K::Number),
            ],
        );
    }
    expect(
        "javascriptreact",
        "const a = <div className=\"x\">{b}</div>;\n",
        &[
            ("div", K::Keyword),
            ("className", K::Variable),
            ("\"x\"", K::String),
        ],
    );
}

#[test]
fn typescript_and_tsx() {
    let text = "interface I { a: string }\nexport type T = number;\nfunction f<T>(a: T): void {}\n";
    for id in ["typescript", "typescriptreact"] {
        expect(
            id,
            text,
            &[
                ("interface", K::Keyword),
                ("I", K::Type),
                ("string", K::Type),
                ("export", K::Keyword),
                ("f", K::Function),
            ],
        );
    }
    expect(
        "typescriptreact",
        "const a = <b c=\"d\"/>;\n",
        &[("b", K::Keyword), ("\"d\"", K::String)],
    );
}

#[test]
fn python() {
    expect(
        "python",
        "# c\ndef f(a, b=1):\n    \"\"\"doc\"\"\"\n    if a: return None\n\nclass C(B): pass\n",
        &[
            ("# c", K::Comment),
            ("def", K::Keyword),
            ("f", K::Function),
            ("1", K::Number),
            ("\"\"\"doc\"\"\"", K::String),
            ("if", K::ControlKeyword),
            ("None", K::Boolean),
            ("C", K::Type),
        ],
    );
}

#[test]
fn c_and_cpp() {
    expect(
        "c",
        "#include <x.h>\nint main(void) { return 0; } // c\nstruct S { int a; };\n",
        &[
            ("#include", K::Keyword),
            ("<x.h>", K::String),
            ("int", K::Type),
            ("main", K::Function),
            ("return", K::ControlKeyword),
            ("0", K::Number),
            ("// c", K::Comment),
            ("struct", K::Keyword),
        ],
    );
    expect(
        "cpp",
        "class A : public B { void f() const; };\nint main() { auto x = 1; std::cout << \"a\"; }\n",
        &[
            ("class", K::Keyword),
            ("A", K::Type),
            ("public", K::Keyword),
            ("f", K::Function),
            ("main", K::Function),
            ("\"a\"", K::String),
        ],
    );
}

#[test]
fn go() {
    expect(
        "go",
        "package main\nfunc main() { x := 1; if x > 0 { fmt.Println(\"a\", nil, true) } }\n// c\n",
        &[
            ("package", K::Keyword),
            ("func", K::Keyword),
            ("main", K::Function),
            ("Println", K::Function),
            ("if", K::ControlKeyword),
            ("nil", K::Boolean),
            ("true", K::Boolean),
            ("\"a\"", K::String),
            ("// c", K::Comment),
        ],
    );
}

#[test]
fn shell() {
    expect(
        "shellscript",
        "#!/bin/sh\nif [ -f x ]; then echo \"a $B\"; fi\nfoo() { ls -l | wc; }\n",
        &[
            ("#!/bin/sh", K::Comment),
            ("if", K::ControlKeyword),
            ("then", K::ControlKeyword),
            ("echo", K::Function),
            ("foo", K::Function),
            ("|", K::Operator),
        ],
    );
}

#[test]
fn yaml() {
    expect(
        "yaml",
        "# c\nkey: value\nlist:\n  - 1\n  - true\n  - null\n",
        &[
            ("# c", K::Comment),
            ("key", K::Keyword),
            ("value", K::String),
            ("1", K::Number),
            ("true", K::Boolean),
            ("null", K::Boolean),
        ],
    );
}

#[test]
fn html() {
    expect(
        "html",
        "<a href=\"x\">t</a><!-- c -->\n",
        &[
            ("a", K::Keyword),
            ("href", K::Variable),
            ("x", K::String),
            ("<!-- c -->", K::Comment),
        ],
    );
}

#[test]
fn css() {
    expect(
        "css",
        "@media screen { b { color: #fff; margin: 1px; } }\n",
        &[
            ("@media", K::Keyword),
            ("b", K::Keyword),
            ("color", K::Variable),
            ("fff", K::String),
            ("1px", K::Number),
        ],
    );
}
