//! Small JSON encoder for read-only command output.

use crate::layout::Rect;

pub fn string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c <= '\u{001f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn number(value: f64) -> String {
    if value.is_finite() {
        value.to_string()
    } else {
        "null".into()
    }
}

pub fn rect(r: Rect) -> String {
    format!(
        "{{\"x\":{},\"y\":{},\"width\":{},\"height\":{}}}",
        number(r.x),
        number(r.y),
        number(r.width),
        number(r.height)
    )
}

pub fn optional(value: Option<&str>) -> String {
    value.map(string).unwrap_or_else(|| "null".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_strings_and_finite_numbers() {
        assert_eq!(string("\"\\\n\t\u{0001}😺"), "\"\\\"\\\\\\n\\t\\u0001😺\"");
        assert_eq!(number(1248.0), "1248");
        assert_eq!(number(1.25), "1.25");
        assert_eq!(number(f64::NAN), "null");
    }

    #[test]
    fn output_is_accepted_by_a_real_json_parser() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let Ok(mut child) = Command::new("python3")
            .args(["-c", "import json,sys;json.load(sys.stdin)"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
        else {
            return;
        };
        let sample = format!(
            "{{\"windows\":[{{\"app\":{},\"title\":null,\"frame\":{}}}]}}",
            string("a\"\\\n😺"),
            rect(Rect::new(1.0, 2.5, 100.0, 200.0))
        );
        child
            .stdin
            .take()
            .unwrap()
            .write_all(sample.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
    }
}
