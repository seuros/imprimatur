use std::assert_matches;
use std::path::PathBuf;

use usage_rs::test::{self as harness, Page};

use super::limits::LimitFlags;
use super::*;

fn parse(argv: &[&str]) -> Result<Imprimatur, String> {
    let words = harness::argv(argv.iter().copied());
    harness::parse(Imprimatur::spec(), &words.words(), Imprimatur::parse_from)
}

#[test]
fn spec_round_trips() {
    let _spec: usage_parser::Spec = Imprimatur::to_kdl().parse().unwrap();
}

#[test]
fn parses_markdown_and_its_alias() {
    let cli = parse(&["--json", "md", "a.pdf", "-", "-o", "out", "--timeout", "2.5"]).unwrap();
    assert!(cli.json);
    let Commands::Markdown(markdown) = cli.command else { panic!("{:?}", cli.command) };
    assert_eq!(markdown.files, [PathBuf::from("a.pdf"), PathBuf::from("-")]);
    assert_eq!(markdown.output, Some(PathBuf::from("out")));
    assert!((markdown.limits.timeout - 2.5).abs() < f64::EPSILON);
    assert_eq!((markdown.limits.max_bytes, markdown.limits.max_inflated), (None, None));

    let cli = parse(&["markdown", "a.xlsx", "--json"]).unwrap();
    assert!(cli.json, "--json is global");
    let Commands::Markdown(markdown) = cli.command else { panic!("{:?}", cli.command) };
    assert_matches!(markdown.limits, LimitFlags { timeout: 60.0, max_bytes: None, .. });
}

#[test]
fn parses_grep_and_images() {
    let argv =
        ["grep", "-i", "-F", "-w", "-C", "2", "-m", "3", "-c", "ISO 17065", "a.pdf", "b.docx"];
    let cli = parse(&[&argv[..], &["--timeout", "5"]].concat()).unwrap();
    let Commands::Grep(grep) = cli.command else { panic!("{:?}", cli.command) };
    assert_eq!(grep.pattern, "ISO 17065");
    assert_eq!(grep.files, [PathBuf::from("a.pdf"), PathBuf::from("b.docx")]);
    let flags = [grep.ignore_case, grep.fixed_strings, grep.word_regexp, grep.count];
    assert_eq!(flags, [true; 4]);
    assert_eq!((grep.files_with_matches, grep.quiet), (false, false));
    assert_eq!((grep.context, grep.max_count), (2, Some(3)));
    assert!((grep.limits.timeout - 5.0).abs() < f64::EPSILON);

    let cli = parse(&["--json", "images", "scan.pdf", "-o", "pages", "--min-size", "600"]).unwrap();
    assert!(cli.json);
    let Commands::Images(images) = cli.command else { panic!("{:?}", cli.command) };
    assert_eq!((images.output, images.min_size), (PathBuf::from("pages"), 600));
    assert_eq!(images.files, [PathBuf::from("scan.pdf")]);
}

#[test]
fn rejects_what_it_cannot_run() {
    const REJECTED: &[(&[&str], &str)] = &[
        (&["markdown"], "FILE"),
        (&["markdown", "a.pdf", "--wat"], "--wat"),
        (&["markdown", "a.pdf", "--timeout", "soon"], "soon"),
        (&["markdown", "a.pdf", "-o", "a", "-o", "b"], "--output"),
        (&["grep", "Total"], "FILE"),
        (&["grep", "-l", "-c", "Total", "a.pdf"], "--count"),
        (&["grep", "-C", "many", "Total", "a.pdf"], "many"),
        (&["images", "a.pdf"], "--output"),
        (&["images", "a.pdf", "-o", "a", "--min-size", "big"], "big"),
        (&["completion", "tcsh"], "tcsh"),
        (&["mark", "a.pdf"], "mark"),
    ];
    for (argv, needle) in REJECTED {
        let message = parse(argv).unwrap_err();
        assert!(message.contains(needle), "argv {argv:?}: {message}");
    }
}

#[test]
fn help_lists_the_exit_codes_and_examples() {
    const PAGES: &[(&str, &[&str])] = &[
        (
            "markdown",
            &["--output", "--timeout", "--max-bytes", "imprimatur md -o notes/", "Exit status:"],
        ),
        ("grep", &["--context", "--files-with-matches", "§Scope", "imprimatur grep -l GOTS"]),
        (
            "images",
            &["--min-size", "NAME-p3-1.png", "imprimatur images -o figures/", "Exit status:"],
        ),
    ];
    let root = harness::help(Imprimatur::spec(), &[], Page::Long);
    assert!(root.contains("no text layer"), "{root}");
    assert!(root.contains("`grep` exits as grep does"), "{root}");
    for (command, needles) in PAGES {
        let page = harness::help(Imprimatur::spec(), &[command], Page::Long);
        for needle in *needles {
            assert!(page.contains(needle), "{command}: {needle}: {page}");
        }
    }
}
