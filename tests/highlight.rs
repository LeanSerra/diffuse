//! Highlighting is checked by rendering the classes under the code, so a wrong
//! bucket is visible rather than merely stable.

use diffuse::highlight::highlight;

fn render(path: &str, source: &str) -> String {
    let lines = highlight(path, source).expect("a grammar for this file");
    let mut out = String::new();
    for (text, spans) in source.lines().zip(&lines) {
        out.push_str(text);
        out.push('\n');
        let units = text.encode_utf16().count();
        let mut bar = vec![' '; units];
        for s in spans {
            for (i, slot) in bar.iter_mut().enumerate().take(s.end.min(units)).skip(s.start) {
                *slot = s.class.chars().nth(i - s.start).unwrap_or('·');
            }
        }
        out.push_str(bar.iter().collect::<String>().trim_end());
        out.push('\n');
    }
    out
}

#[test]
fn classifies_rust() {
    insta::assert_snapshot!(render(
        "a.rs",
        "// note\nfn main() {\n    let x = 42;\n    let s = \"hi\";\n}\n"
    ));
}

/// The reason whole-file context exists: a hunk can start inside a block
/// comment, and per-line highlighting would render this as live code.
#[test]
fn a_line_inside_a_block_comment_is_still_a_comment() {
    let source = "/*\nlet x = 42;\n*/\nlet y = 1;\n";
    let lines = highlight("a.rs", source).unwrap();
    let inside = &lines[1];
    assert!(
        inside.iter().all(|s| s.class == "com"),
        "line 2 sits inside a block comment, got {inside:?}",
    );
    assert!(
        lines[3].iter().any(|s| s.class == "kw"),
        "line 4 is back in code, got {:?}", lines[3],
    );
}

#[test]
fn offsets_are_utf16() {
    // The emoji is two UTF-16 units; a char-index offset would shift the string.
    let lines = highlight("a.rs", "let s = \"🎉 hi\";\n").unwrap();
    let text = "let s = \"🎉 hi\";";
    let units: Vec<u16> = text.encode_utf16().collect();
    let string = lines[0].iter().find(|s| s.class == "str").expect("a string span");
    let slice = String::from_utf16_lossy(&units[string.start..string.end]);
    assert!(slice.starts_with('"') && slice.contains("🎉"), "got {slice:?}");
}

#[test]
fn unknown_extensions_are_left_plain() {
    assert!(highlight("a.wat-is-this", "hello\n").is_none());
}

#[test]
fn covers_the_languages_in_use_here() {
    for (path, src) in [
        ("a.rs", "fn f() {}\n"),
        ("a.tsx", "const A = () => <div className=\"x\" />;\n"),
        ("a.ts", "export const x: number = 1;\n"),
        ("a.py", "def f():\n    return 1\n"),
        ("a.cpp", "int main() { return 0; }\n"),
        ("a.json", "{\"a\": 1}\n"),
        ("a.toml", "[pkg]\nname = \"x\"\n"),
        ("a.css", ".a { color: red; }\n"),
        ("a.md", "# hi\n"),
        ("a.sh", "echo hi\n"),
    ] {
        let got = highlight(path, src);
        assert!(got.is_some(), "no grammar for {path}");
        assert!(
            got.unwrap().iter().any(|l| !l.is_empty()),
            "{path} produced no spans at all",
        );
    }
}

/// The complaints that prompted the palette rework: in C, types and functions
/// must not share a class, constants need one at all, and operators must not
/// be painted like keywords.
#[test]
fn c_distinguishes_types_functions_constants_and_operators() {
    let src = "#define MAX 10\nstatic int add(int a, int b) {\n    return a + b;\n}\n";
    let lines = highlight("a.c", src).unwrap();
    let class_at = |line: usize, needle: &str| -> Option<String> {
        let text = src.lines().nth(line)?;
        let at = text.find(needle)? as usize;
        let at16 = text[..at].encode_utf16().count();
        lines[line]
            .iter()
            .find(|s| s.start <= at16 && at16 < s.end)
            .map(|s| s.class.to_string())
    };
    let ty = class_at(1, "int").expect("int is classified");
    let func = class_at(1, "add").expect("add is classified");
    assert_ne!(ty, func, "a type and a function must not share a class");
    assert_eq!(class_at(2, "+"), None, "operators are left plain");
    assert_eq!(class_at(1, "("), None, "punctuation is left plain");
    assert_eq!(class_at(1, "static").as_deref(), Some("kw"));
}
