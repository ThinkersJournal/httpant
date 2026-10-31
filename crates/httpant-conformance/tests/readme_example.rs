// SPDX-License-Identifier: MIT OR Apache-2.0
//! The README's first example, `examples/hello.rs` and the hub card (`easyants-card.toml`) are one program in three places. This test keeps them one:
//! the README block is lines 2 to 11 of the example, and the card names exactly those lines, at most ten. It lives here, not in the `httpant` package,
//! because it reads files of the repository that the published package deliberately does not contain.

fn root(path: &str) -> String {
    format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))
}

fn lines(path: &str) -> Vec<String> {
    let path = root(path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .replace("\r\n", "\n")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_readme_example_is_the_example_file_and_the_card_shows_it() {
    let readme = lines("README.md");
    let open = readme
        .iter()
        .position(|l| l == "```rust")
        .expect("README has a rust block");
    let close = readme[open + 1..]
        .iter()
        .position(|l| l == "```")
        .expect("the block closes")
        + open
        + 1;
    let in_readme = &readme[open + 1..close];

    let example = lines("examples/hello.rs");
    assert_eq!(
        in_readme,
        &example[1..],
        "README's first example must be examples/hello.rs without its licence line"
    );

    let card = std::fs::read_to_string(root("easyants-card.toml")).expect("the hub card");
    assert!(
        card.contains("example_path = \"examples/hello.rs\""),
        "the card must name examples/hello.rs"
    );
    let want = format!("example_lines = [2, {}]", example.len());
    assert!(
        card.contains(&want),
        "the card must show lines 2 to {}: expected `{want}`",
        example.len()
    );
    assert!(
        in_readme.len() <= 10,
        "the hub shows at most 10 lines; the example has {}",
        in_readme.len()
    );
}
