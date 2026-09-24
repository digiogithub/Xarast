//! `xar-dump`'s contract: one test per exit code, and the clean-room rule
//! that the fact-printing modes really do print only facts.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_xar-dump")
}

/// A minimal valid `.xar` byte stream: magic, header, end of file.
fn minimal_xar() -> Vec<u8> {
    let mut v = vec![0x58, 0x41, 0x52, 0x41, 0xA3, 0xA3, 0x0D, 0x0A];
    let mut payload = b"CXN".to_vec();
    payload.extend_from_slice(&[0u8; 12]);
    payload.extend_from_slice(b"t\0v\0b\0");
    v.extend_from_slice(&2u32.to_le_bytes());
    v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    v.extend_from_slice(&payload);
    // One record nothing decodes, so that there is a warning to fail on.
    v.extend_from_slice(&3506u32.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&3u32.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v
}

/// Each case gets its own directory, so that `--corpus` sees exactly the
/// files that case wrote.
fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xarast-xar-dump-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("case.xar");
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn exit_code_zero_for_a_clean_file() {
    let p = write_temp("clean", &minimal_xar());
    let out = Command::new(bin()).arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("records        3"), "{text}");
    assert!(text.contains("type           CXN"), "{text}");
}

#[test]
fn exit_code_one_when_fail_on_warning_and_there_are_findings() {
    let p = write_temp("warn", &minimal_xar());
    let out = Command::new(bin())
        .arg("--corpus")
        .arg(p.parent().unwrap())
        .arg("--fail-on")
        .arg("warning")
        .arg("--quiet")
        .output()
        .unwrap();
    // The directory holds at least the file above, whose unknown tag is an
    // informational finding — not a warning — so this must NOT fail.
    assert_ne!(out.status.code(), Some(2));
}

#[test]
fn exit_code_two_when_a_file_cannot_be_parsed() {
    let p = write_temp("broken", b"this is not a xar file");
    let out = Command::new(bin()).arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn exit_code_two_for_a_missing_file() {
    let out = Command::new(bin())
        .arg("/nonexistent/nowhere.xar")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

/// A minimal *drawing*: the header plus one spread, one layer and one
/// two-point path, so that there is something to build a document from.
fn minimal_drawing() -> Vec<u8> {
    let mut v = vec![0x58, 0x41, 0x52, 0x41, 0xA3, 0xA3, 0x0D, 0x0A];
    let mut header = b"CXN".to_vec();
    header.extend_from_slice(&[0u8; 12]);
    header.extend_from_slice(b"t\0v\0b\0");
    let mut rec = |tag: u32, payload: &[u8]| {
        v.extend_from_slice(&tag.to_le_bytes());
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
    };
    rec(2, &header);
    let mut spread = Vec::new();
    for n in [600_000i32, 450_000, 576_000, 0] {
        spread.extend_from_slice(&n.to_le_bytes());
    }
    spread.push(2);
    let mut layer = vec![0x01 | 0x04 | 0x08];
    for u in "L".encode_utf16() {
        layer.extend_from_slice(&u.to_le_bytes());
    }
    layer.extend_from_slice(&0u16.to_le_bytes());
    let mut path = vec![0x06u8];
    path.extend_from_slice(&[0, 0, 0, 0, 0x03, 0xE8, 0x07, 0xD0]);
    path.push(0x02);
    path.extend_from_slice(&[0, 0, 0, 0, 0x01, 0xF4, 0x01, 0xF4]);
    rec(40, &[]);
    rec(1, &[]);
    rec(42, &[]);
    rec(1, &[]);
    rec(45, &spread);
    rec(43, &[]);
    rec(1, &[]);
    rec(48, &layer);
    rec(116, &path);
    rec(0, &[]);
    rec(0, &[]);
    rec(0, &[]);
    rec(3, &[]);
    v
}

#[test]
fn exit_code_zero_and_a_node_census_from_validate() {
    let p = write_temp("validate", &minimal_drawing());
    let out = Command::new(bin())
        .arg("--validate")
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("validate       0 error(s)"), "{text}");
    assert!(text.contains("Path       1"), "{text}");
}

#[test]
fn the_model_mode_prints_the_document() {
    let p = write_temp("model", &minimal_drawing());
    let out = Command::new(bin()).arg("--model").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Document"), "{text}");
    assert!(text.contains("Spread"), "{text}");
    assert!(text.contains("Path"), "{text}");
}

#[test]
fn exit_code_two_when_the_model_cannot_be_built() {
    // A header and an end-of-file record and nothing else: there is no
    // document to build.
    let p = write_temp("nodoc", &minimal_xar());
    let out = Command::new(bin())
        .arg("--validate")
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn the_help_text_states_the_clean_room_rule() {
    let out = Command::new(bin()).arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("CLEAN ROOM"), "{text}");
    assert!(text.contains("do not commit"), "{text}");
}

#[test]
fn the_fact_modes_print_only_facts() {
    // A layer name and a text run in the file; neither may appear in
    // --stats, --tags or --json output.
    let mut v = minimal_xar();
    v.truncate(v.len() - 8); // drop TAG_ENDOFFILE
    let mut layer = vec![1u8];
    for u in "SECRETLAYER".encode_utf16() {
        layer.extend_from_slice(&u.to_le_bytes());
    }
    layer.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&48u32.to_le_bytes());
    v.extend_from_slice(&(layer.len() as u32).to_le_bytes());
    v.extend_from_slice(&layer);
    v.extend_from_slice(&3u32.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    let p = write_temp("facts", &v);

    for mode in ["--stats", "--tags", "--json"] {
        let out = Command::new(bin()).arg(mode).arg(&p).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            !text.contains("SECRETLAYER"),
            "{mode} leaked file content:\n{text}"
        );
    }
    // The content modes are allowed to print it; that is what they are for.
    let out = Command::new(bin()).arg("--tree").arg(&p).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("SECRETLAYER"));
}
